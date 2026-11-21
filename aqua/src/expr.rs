//! Strict evaluation of the Aqua version expressions.
//!
//! Aqua uses the `expr` language for `version_constraint`, `version_overrides`,
//! and `version_filter`. This module accepts the subset that appears in real
//! registry recipes and rejects everything else rather than guessing. It never
//! includes the expression text in an error, so a registry value cannot leak
//! through a message.

use crate::version;

/// Values available to an expression.
#[derive(Debug)]
pub(crate) struct Context<'a> {
    /// The exact package version.
    pub version: &'a str,
    /// The version used by the `semver` function.
    pub semver: &'a str,
    /// Whether the `SemVer` variable is available.
    pub allow_semver_variable: bool,
}

/// A token in the supported expression subset.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Open,
    Close,
    OpenList,
    CloseList,
    Comma,
    Equal,
    NotEqual,
    Not,
    And,
    Or,
}

/// Tokenize an expression, rejecting characters outside the subset.
fn tokenize(input: &str) -> Result<Vec<Token>, ()> {
    let mut chars = input.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        match ch {
            ch if ch.is_whitespace() => {}
            '(' => tokens.push(Token::Open),
            ')' => tokens.push(Token::Close),
            '[' => tokens.push(Token::OpenList),
            ']' => tokens.push(Token::CloseList),
            ',' => tokens.push(Token::Comma),
            '=' => {
                if chars.next_if_eq(&'=').is_some() {
                    tokens.push(Token::Equal);
                } else {
                    // A single `=` is not an expr operator.
                    return Err(());
                }
            }
            '!' => {
                if chars.next_if_eq(&'=').is_some() {
                    tokens.push(Token::NotEqual);
                } else {
                    tokens.push(Token::Not);
                }
            }
            '&' => {
                if chars.next_if_eq(&'&').is_some() {
                    tokens.push(Token::And);
                } else {
                    return Err(());
                }
            }
            '|' => {
                if chars.next_if_eq(&'|').is_some() {
                    tokens.push(Token::Or);
                } else {
                    return Err(());
                }
            }
            '"' => tokens.push(Token::Str(read_string(&mut chars)?)),
            ch if ch.is_ascii_alphabetic() || ch == '_' => {
                let mut ident = String::new();
                ident.push(ch);
                while let Some(&next) = chars.peek()
                    && (next.is_ascii_alphanumeric() || next == '_')
                {
                    ident.push(next);
                    chars.next();
                }
                tokens.push(Token::Ident(ident));
            }
            _ => return Err(()),
        }
    }
    Ok(tokens)
}

/// Read a double-quoted string with Go-style backslash escapes.
fn read_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String, ()> {
    let mut value = String::new();
    loop {
        match chars.next() {
            None => return Err(()),
            Some('"') => return Ok(value),
            Some('\\') => {
                let escaped = chars.next().ok_or(())?;
                value.push(match escaped {
                    '"' => '"',
                    '\\' => '\\',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return Err(()),
                });
            }
            Some(ch) => value.push(ch),
        }
    }
}

/// Parser and evaluator for one expression.
struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    context: &'a Context<'a>,
}

impl Parser<'_> {
    /// The token at the cursor, without consuming it.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    /// Consume the token at the cursor if it is `expected`.
    fn take(&mut self, expected: &Token) -> Result<(), ()> {
        if self.peek() == Some(expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    /// Test whether the cursor holds the identifier `name`.
    fn at_ident(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Token::Ident(value)) if value == name)
    }

    /// Test whether the cursor holds `token`.
    fn at(&self, token: &Token) -> bool {
        self.peek() == Some(token)
    }

    /// Parse the whole expression, which must be a boolean.
    fn parse(mut self) -> Result<bool, ()> {
        let value = self.parse_or()?;
        if self.position == self.tokens.len() {
            Ok(value)
        } else {
            Err(())
        }
    }

    /// Parse `a or b` and `a || b`.
    fn parse_or(&mut self) -> Result<bool, ()> {
        let mut value = self.parse_and()?;
        loop {
            if self.at(&Token::Or) || self.at_ident("or") {
                self.position += 1;
                let right = self.parse_and()?;
                value = value || right;
            } else {
                return Ok(value);
            }
        }
    }

    /// Parse `a and b` and `a && b`.
    fn parse_and(&mut self) -> Result<bool, ()> {
        let mut value = self.parse_not()?;
        loop {
            if self.at(&Token::And) || self.at_ident("and") {
                self.position += 1;
                let right = self.parse_not()?;
                value = value && right;
            } else {
                return Ok(value);
            }
        }
    }

    /// Parse a negated value.
    fn parse_not(&mut self) -> Result<bool, ()> {
        if self.at(&Token::Not) || self.at_ident("not") {
            self.position += 1;
            return Ok(!self.parse_not()?);
        }
        self.parse_primary()
    }

    /// Parse a comparison, function call, literal, or parenthesized group.
    fn parse_primary(&mut self) -> Result<bool, ()> {
        if self.at(&Token::Open) {
            self.position += 1;
            let value = self.parse_or()?;
            self.take(&Token::Close)?;
            return Ok(value);
        }
        if self.at_ident("true") {
            self.position += 1;
            return Ok(true);
        }
        if self.at_ident("false") {
            self.position += 1;
            return Ok(false);
        }
        if self.at_ident("semver") {
            self.position += 1;
            return self.parse_semver(false);
        }
        if self.at_ident("semverWithVersion") {
            self.position += 1;
            return self.parse_semver(true);
        }
        let left = self.parse_value()?;
        if self.at(&Token::Equal) {
            self.position += 1;
            return Ok(left == self.parse_value()?);
        }
        if self.at(&Token::NotEqual) {
            self.position += 1;
            return Ok(left != self.parse_value()?);
        }
        if self.at_ident("in") {
            self.position += 1;
            return self.parse_list(&left);
        }
        for name in ["startsWith", "endsWith", "contains"] {
            if self.at_ident(name) {
                self.position += 1;
                let right = self.parse_value()?;
                return Ok(match name {
                    "startsWith" => left.starts_with(&right),
                    "endsWith" => left.ends_with(&right),
                    _ => left.contains(&right),
                });
            }
        }
        if self.at_ident("matches") {
            self.position += 1;
            let pattern = self.parse_value()?;
            let regex = regex::Regex::new(&pattern).map_err(|_| ())?;
            return Ok(regex.is_match(&left));
        }
        Err(())
    }

    /// Parse a `semver` or `semverWithVersion` call.
    fn parse_semver(&mut self, with_version: bool) -> Result<bool, ()> {
        self.take(&Token::Open)?;
        let constraint = self.parse_value()?;
        let version = if with_version {
            self.take(&Token::Comma)?;
            self.parse_value()?
        } else {
            self.context.semver.to_owned()
        };
        self.take(&Token::Close)?;
        version::satisfies(&version, &constraint).map_err(|_| ())
    }

    /// Parse a value, which may be a literal or a string function.
    fn parse_value(&mut self) -> Result<String, ()> {
        match self.peek().cloned() {
            Some(Token::Str(value)) => {
                self.position += 1;
                Ok(value)
            }
            Some(Token::Ident(name)) if name == "Version" => {
                self.position += 1;
                Ok(self.context.version.to_owned())
            }
            Some(Token::Ident(name)) if name == "SemVer" => {
                if !self.context.allow_semver_variable {
                    return Err(());
                }
                self.position += 1;
                Ok(self.context.semver.to_owned())
            }
            Some(Token::Ident(name)) => {
                self.position += 1;
                match name.as_str() {
                    "trimV" => {
                        self.take(&Token::Open)?;
                        let value = self.parse_value()?;
                        self.take(&Token::Close)?;
                        Ok(value.strip_prefix('v').unwrap_or(&value).to_owned())
                    }
                    "trimPrefix" | "trimSuffix" => {
                        self.take(&Token::Open)?;
                        // Aqua's expr functions follow Go's strings package:
                        // `trimPrefix(s, prefix)`, not sprig's `(prefix, s)`.
                        let value = self.parse_value()?;
                        self.take(&Token::Comma)?;
                        let affix = self.parse_value()?;
                        let value = if name == "trimPrefix" {
                            value.strip_prefix(&affix).unwrap_or(&value).to_owned()
                        } else {
                            value.strip_suffix(&affix).unwrap_or(&value).to_owned()
                        };
                        self.take(&Token::Close)?;
                        Ok(value)
                    }
                    _ => Err(()),
                }
            }
            _ => Err(()),
        }
    }

    /// Parse `in [ ... ]`.
    fn parse_list(&mut self, left: &str) -> Result<bool, ()> {
        self.take(&Token::OpenList)?;
        let mut found = false;
        loop {
            if self.at(&Token::CloseList) {
                self.position += 1;
                return Ok(found);
            }
            let value = self.parse_value()?;
            found |= value == left;
            if self.at(&Token::Comma) {
                self.position += 1;
            } else {
                self.take(&Token::CloseList)?;
                return Ok(found);
            }
        }
    }
}

/// Evaluate a supported Aqua expression as a boolean.
pub(crate) fn evaluate(input: &str, context: &Context<'_>) -> Result<bool, ()> {
    let tokens = tokenize(input)?;
    Parser {
        tokens,
        position: 0,
        context,
    }
    .parse()
}

#[cfg(test)]
mod tests {
    use super::{Context, evaluate};

    fn context<'a>(version: &'a str, semver: &'a str) -> Context<'a> {
        Context {
            version,
            semver,
            allow_semver_variable: true,
        }
    }

    #[test]
    fn evaluates_literals_and_semver() {
        let context = context("v1.2.3", "v1.2.3");
        assert_eq!(evaluate("true", &context), Ok(true));
        assert_eq!(evaluate("false", &context), Ok(false));
        assert_eq!(evaluate(r#"semver(">= 1.0.0")"#, &context), Ok(true));
        assert_eq!(evaluate(r#"semver("< 1.0.0")"#, &context), Ok(false));
        assert_eq!(
            evaluate(r#"semver(">= 1.0.0, < 2.0.0")"#, &context),
            Ok(true)
        );
    }

    #[test]
    fn evaluates_version_comparisons_and_lists() {
        let context = context("v0.1.0", "v0.1.0");
        assert_eq!(evaluate(r#"Version == "v0.1.0""#, &context), Ok(true));
        assert_eq!(evaluate(r#"Version != "v0.1.0""#, &context), Ok(false));
        assert_eq!(
            evaluate(r#"Version in ["v0.1.0", "v0.2.0"]"#, &context),
            Ok(true)
        );
        assert_eq!(
            evaluate(r#"Version in ["v0.2.0", "v0.3.0"]"#, &context),
            Ok(false)
        );
        assert_eq!(evaluate(r#"Version startsWith "v0.1""#, &context), Ok(true));
        assert_eq!(evaluate(r#"Version contains ".1.0""#, &context), Ok(true));
        assert_eq!(
            evaluate(r#"Version matches "^v[0-9]+\\.[0-9]+\\.[0-9]+$""#, &context),
            Ok(true)
        );
    }

    #[test]
    fn evaluates_logical_operators() {
        let context = context("v2.0.0", "v2.0.0");
        assert_eq!(
            evaluate(
                r#"not (Version startsWith "v1") and semver(">= 2.0.0")"#,
                &context
            ),
            Ok(true)
        );
        assert_eq!(
            evaluate(r#"semver("< 2.0.0") or Version in ["v2.0.0"]"#, &context),
            Ok(true)
        );
        assert_eq!(
            evaluate(r#"semver("< 2.0.0") || Version == "v2.0.0""#, &context),
            Ok(true)
        );
        assert_eq!(evaluate(r#"!(Version == "v2.0.0")"#, &context), Ok(false));
    }

    #[test]
    fn evaluates_semver_with_version_and_prefix_functions() {
        let release = context("release-cli-0.6.0", "release-cli-0.6.0");
        assert_eq!(
            evaluate(
                r#"semverWithVersion(">= 0.6.0", trimPrefix(Version, "release-cli-"))"#,
                &release
            ),
            Ok(true)
        );
        assert_eq!(
            evaluate(r#"trimV(Version) == "release-cli-0.6.0""#, &release),
            Ok(true)
        );
        let prefixed = context("v1.2.3", "v1.2.3");
        assert_eq!(
            evaluate(r#"trimPrefix(Version, "v") == "1.2.3""#, &prefixed),
            Ok(true)
        );
        assert_eq!(
            evaluate(r#"trimSuffix(Version, ".3") == "v1.2""#, &prefixed),
            Ok(true)
        );
    }

    #[test]
    fn rejects_unsupported_expressions() {
        let context = context("v1.0.0", "v1.0.0");
        for expression in [
            r#"semver(">= 1.0.0") ? true : false"#,
            r#"Version =~ "^v""#,
            r#"Version.foo == "v1.0.0""#,
            r#"semver(">= 1.0.0""#,
            r#"Version == "#,
            r#"unknownThing > 1"#,
            r#"Version matches "[""#,
            r#"true false"#,
        ] {
            assert!(evaluate(expression, &context).is_err(), "{expression}");
        }
    }

    #[test]
    fn hides_semver_variable_when_disabled() {
        let context = Context {
            version: "v1.0.0",
            semver: "1.0.0",
            allow_semver_variable: false,
        };
        assert_eq!(evaluate(r#"SemVer == "1.0.0""#, &context), Err(()));
        assert_eq!(
            evaluate(
                r#"semverWithVersion(">= 1.0.0", trimPrefix(Version, "v"))"#,
                &context
            ),
            Ok(true)
        );
    }
}
