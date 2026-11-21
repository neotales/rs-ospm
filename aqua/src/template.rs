//! Strict renderer for the Aqua template subset.
//!
//! Aqua renders template strings with Go's `text/template` and a trimmed set of
//! sprig functions. This module supports the variables and functions that the
//! registry actually uses for release assets, file paths, and checksum files.
//! Anything else is rejected, including the structured `semver` helpers and
//! environment access.

/// Values available while rendering a template.
#[derive(Debug)]
pub(crate) struct Context<'a> {
    /// Mapped operating-system value, used by `.OS`.
    pub os: &'a str,
    /// Mapped architecture value, used by `.Arch`.
    pub arch: &'a str,
    /// Native Go operating system, used by `.GOOS`.
    pub goos: &'a str,
    /// Native Go architecture, used by `.GOARCH`.
    pub goarch: &'a str,
    /// Exact package version, used by `.Version`.
    pub version: &'a str,
    /// Version after the recipe's prefix is trimmed, used by `.SemVer`.
    pub semver: &'a str,
    /// Effective archive format, used by `.Format`.
    pub format: &'a str,
    /// Rendered asset name, when available.
    pub asset: Option<&'a str>,
    /// Asset name with a known archive extension removed.
    pub asset_without_ext: Option<&'a str>,
    /// Current file name, when rendering a file source.
    pub file_name: Option<&'a str>,
}

/// One token in a template action.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Pipe,
}

/// Tokenize the text between `{{` and `}}`.
fn tokenize(input: &str) -> Result<Vec<Token>, ()> {
    let mut chars = input.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        match ch {
            ch if ch.is_whitespace() => {}
            '|' => tokens.push(Token::Pipe),
            '"' => tokens.push(Token::Str(read_string(&mut chars)?)),
            ch if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' => {
                let mut ident = String::new();
                ident.push(ch);
                while let Some(&next) = chars.peek()
                    && (next.is_ascii_alphanumeric() || next == '_' || next == '.')
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

/// Evaluate one command, optionally with a piped-in value.
fn command(tokens: &[Token], piped: Option<&str>, context: &Context<'_>) -> Result<String, ()> {
    if tokens.is_empty() {
        return Err(());
    }
    if let [Token::Ident(name)] = tokens
        && name.starts_with('.')
    {
        if piped.is_some() {
            return Err(());
        }
        return variable(name, context);
    }
    if let [Token::Str(value)] = tokens {
        if piped.is_some() {
            return Err(());
        }
        return Ok(value.clone());
    }
    let Token::Ident(function) = &tokens[0] else {
        return Err(());
    };
    if function.starts_with('.') {
        return Err(());
    }
    let mut arguments = Vec::new();
    for token in &tokens[1..] {
        match token {
            Token::Str(value) => arguments.push(value.clone()),
            Token::Ident(name) if name.starts_with('.') => {
                arguments.push(variable(name, context)?);
            }
            _ => return Err(()),
        }
    }
    if let Some(value) = piped {
        arguments.push(value.to_owned());
    }
    apply(function, &arguments)
}

/// Look up a template variable.
fn variable(name: &str, context: &Context<'_>) -> Result<String, ()> {
    match name {
        ".OS" => Ok(context.os.to_owned()),
        ".Arch" => Ok(context.arch.to_owned()),
        ".GOOS" => Ok(context.goos.to_owned()),
        ".GOARCH" => Ok(context.goarch.to_owned()),
        ".Version" => Ok(context.version.to_owned()),
        ".SemVer" => Ok(context.semver.to_owned()),
        ".Format" => Ok(context.format.to_owned()),
        ".Asset" => context.asset.map(str::to_owned).ok_or(()),
        ".AssetWithoutExt" => context.asset_without_ext.map(str::to_owned).ok_or(()),
        ".FileName" => context.file_name.map(str::to_owned).ok_or(()),
        _ => Err(()),
    }
}

/// Apply a supported template function.
fn apply(function: &str, arguments: &[String]) -> Result<String, ()> {
    match (function, arguments) {
        ("trimV", [value]) => Ok(value.strip_prefix('v').unwrap_or(value).to_owned()),
        ("trimPrefix", [prefix, value]) => {
            Ok(value.strip_prefix(prefix).unwrap_or(value).to_owned())
        }
        ("trimSuffix", [suffix, value]) => {
            Ok(value.strip_suffix(suffix).unwrap_or(value).to_owned())
        }
        ("replace", [old, new, value]) => Ok(value.replace(old.as_str(), new)),
        ("title", [value]) => Ok(value
            .split(' ')
            .map(|word| {
                let mut characters = word.chars();
                match characters.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")),
        ("lower" | "toLower", [value]) => Ok(value.to_lowercase()),
        ("upper" | "toUpper", [value]) => Ok(value.to_uppercase()),
        ("trim", [value]) => Ok(value.trim().to_owned()),
        _ => Err(()),
    }
}

/// Render a template string, rejecting unsupported actions.
pub(crate) fn render(input: &str, context: &Context<'_>) -> Result<String, ()> {
    let mut output = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(());
        };
        let action = &after[..end];
        let tokens = tokenize(action)?;
        let mut commands = tokens.split(|token| token == &Token::Pipe);
        let first = commands.next().ok_or(())?;
        let mut value = command(first, None, context)?;
        for next in commands {
            value = command(next, Some(&value), context)?;
        }
        output.push_str(&value);
        rest = &after[end + 2..];
    }
    if rest.contains("}}") {
        return Err(());
    }
    output.push_str(rest);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{Context, render};

    fn context<'a>() -> Context<'a> {
        Context {
            os: "darwin",
            arch: "amd64",
            goos: "darwin",
            goarch: "arm64",
            version: "v1.2.3",
            semver: "1.2.3",
            format: "tar.gz",
            asset: Some("tool_1.2.3_darwin_amd64.tar.gz"),
            asset_without_ext: Some("tool_1.2.3_darwin_amd64"),
            file_name: Some("tool"),
        }
    }

    #[test]
    fn renders_variables_and_common_functions() {
        let context = context();
        assert_eq!(
            render(
                "tool_{{trimV .Version}}_{{.OS}}_{{.Arch}}.{{.Format}}",
                &context
            ),
            Ok("tool_1.2.3_darwin_amd64.tar.gz".to_owned())
        );
        assert_eq!(
            render("{{.AssetWithoutExt}}/bin/{{.FileName}}", &context),
            Ok("tool_1.2.3_darwin_amd64/bin/tool".to_owned())
        );
        assert_eq!(
            render(r#"{{trimPrefix "cli-" .Version}}"#, &context),
            Ok("v1.2.3".to_owned())
        );
        assert_eq!(render("{{.SemVer}}", &context), Ok("1.2.3".to_owned()));
        assert_eq!(render("{{.GOARCH}}", &context), Ok("arm64".to_owned()));
    }

    #[test]
    fn renders_pipelines() {
        let context = Context {
            version: "rc/v1.2.3",
            semver: "rc/v1.2.3",
            ..context()
        };
        assert_eq!(
            render(r#"{{trimPrefix "rc/" .Version | trimV}}"#, &context),
            Ok("1.2.3".to_owned())
        );
        assert_eq!(
            render(r#"{{.AssetWithoutExt | trimSuffix "_amd64"}}"#, &context),
            Ok("tool_1.2.3_darwin".to_owned())
        );
    }

    #[test]
    fn rejects_unsupported_actions() {
        let context = context();
        for template in [
            "{{.Unknown}}",
            "{{.Vars.channel}}",
            r#"{{(semver .Version).Major}}"#,
            r#"{{env "HOME"}}"#,
            "{{if .OS}}x{{end}}",
            "{{.OS",
            "{{}}",
        ] {
            assert!(render(template, &context).is_err(), "{template}");
        }
    }
}
