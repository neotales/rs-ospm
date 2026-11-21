//! Version parsing and Aqua `semver` constraint evaluation.
//!
//! Aqua evaluates version constraints with `hashicorp/go-version`. This module
//! implements the subset needed to compare ordinary release tags: numeric
//! segments, prerelease identifiers, build metadata, and the comparison
//! operators `>=`, `<=`, `!=`, `>`, `<`, and `=`. Range comparisons apply the
//! library's `prereleaseCheck` rule.

use std::cmp::Ordering;

/// One prerelease identifier.
#[derive(Debug, Eq, PartialEq)]
enum Identifier {
    /// A numeric identifier, compared numerically.
    Number(u64),
    /// A non-numeric identifier, compared lexically.
    Text(String),
}

/// A parsed version with numeric segments and an optional prerelease.
#[derive(Debug, Eq, PartialEq)]
struct Version {
    segments: Vec<u64>,
    prerelease: Vec<Identifier>,
}

impl Version {
    /// Compare two versions using `hashicorp/go-version` conventions.
    fn compare(&self, other: &Self) -> Ordering {
        let segments = self.segments.len().max(other.segments.len());
        for index in 0..segments {
            let left = self.segments.get(index).copied().unwrap_or(0);
            let right = other.segments.get(index).copied().unwrap_or(0);
            match left.cmp(&right) {
                Ordering::Equal => {}
                order => return order,
            }
        }
        match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => compare_prerelease(&self.prerelease, &other.prerelease),
        }
    }

    /// Test whether both versions have identical numeric base segments.
    ///
    /// `hashicorp/go-version` uses this when a constraint and a version both
    /// carry prerelease data.
    fn equal_segments(&self, other: &Self) -> bool {
        self.segments == other.segments
    }
}

/// Apply `hashicorp/go-version`'s `prereleaseCheck` rule.
///
/// A range constraint without a prerelease does not match a prerelease
/// version. Two prereleases only match when their numeric base segments are
/// identical. A prerelease constraint still matches a non-prerelease version.
fn prerelease_check(version: &Version, constraint: &Version) -> bool {
    match (
        version.prerelease.is_empty(),
        constraint.prerelease.is_empty(),
    ) {
        (false, false) => version.equal_segments(constraint),
        (false, true) => false,
        (true, false) | (true, true) => true,
    }
}

/// Compare prerelease identifier lists.
fn compare_prerelease(left: &[Identifier], right: &[Identifier]) -> Ordering {
    for (a, b) in left.iter().zip(right) {
        let order = match (a, b) {
            (Identifier::Number(a), Identifier::Number(b)) => a.cmp(b),
            (Identifier::Number(_), Identifier::Text(_)) => Ordering::Less,
            (Identifier::Text(_), Identifier::Number(_)) => Ordering::Greater,
            (Identifier::Text(a), Identifier::Text(b)) => a.cmp(b),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

/// Parse a release tag as a version.
///
/// An optional leading `v` is accepted, as are any number of numeric segments,
/// prerelease identifiers, and build metadata. A 40-character commit hash is
/// not a version, matching Aqua.
fn parse(input: &str) -> Option<Version> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.len() == 40 && input.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let input = input
        .strip_prefix('v')
        .or_else(|| input.strip_prefix('V'))
        .unwrap_or(input);
    let input = input.split_once('+').map_or(input, |(core, _)| core);
    let (core, prerelease) = match input.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease.to_owned())),
        None => (input, None),
    };
    if core.is_empty() {
        return None;
    }
    // `hashicorp/go-version` also reads a prerelease without a hyphen, as in
    // `1.2.0b3`. Split at the first character that cannot be part of the
    // numeric core.
    let (core, inline_prerelease) =
        match core.find(|character: char| !character.is_ascii_digit() && character != '.') {
            Some(index) if prerelease.is_none() => (
                core[..index].trim_end_matches('.'),
                Some(core[index..].to_owned()),
            ),
            Some(_) => return None,
            None => (core, None),
        };
    let prerelease = prerelease.or(inline_prerelease);
    let mut segments = Vec::new();
    for segment in core.split('.') {
        if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        segments.push(segment.parse().ok()?);
    }
    // `hashicorp/go-version` pads fewer than three segments with zeros before
    // comparing prerelease base segments.
    while segments.len() < 3 {
        segments.push(0);
    }
    let prerelease = match prerelease {
        None => Vec::new(),
        Some(value) => {
            if value.is_empty() {
                return None;
            }
            let mut identifiers = Vec::new();
            for part in value.split('.') {
                if part.is_empty()
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'~')
                {
                    return None;
                }
                if part.bytes().all(|byte| byte.is_ascii_digit()) {
                    identifiers.push(Identifier::Number(part.parse().ok()?));
                } else {
                    identifiers.push(Identifier::Text(part.to_owned()));
                }
            }
            identifiers
        }
    };
    Some(Version {
        segments,
        prerelease,
    })
}

/// Test a version against an Aqua `semver` constraint string.
///
/// A constraint is one or more `<operator> <version>` pairs separated by
/// commas; every pair must hold. Range operators apply
/// `hashicorp/go-version`'s prerelease rule, while `=` and `!=` compare
/// directly. An unparseable *version* does not satisfy any constraint,
/// matching Aqua. A malformed constraint is an error so that a broken recipe is
/// reported rather than treated as a version mismatch.
pub(crate) fn satisfies(version: &str, constraint: &str) -> Result<bool, ()> {
    let Some(version) = parse(version) else {
        return Ok(false);
    };
    if constraint.trim().is_empty() {
        return Err(());
    }
    for part in constraint.split(',') {
        let part = part.trim();
        let (operator, rest) = if let Some(rest) = part.strip_prefix(">=") {
            (">=", rest)
        } else if let Some(rest) = part.strip_prefix("<=") {
            ("<=", rest)
        } else if let Some(rest) = part.strip_prefix("!=") {
            ("!=", rest)
        } else if let Some(rest) = part.strip_prefix('>') {
            (">", rest)
        } else if let Some(rest) = part.strip_prefix('<') {
            ("<", rest)
        } else if let Some(rest) = part.strip_prefix('=') {
            ("=", rest)
        } else {
            return Err(());
        };
        let rest = rest.trim();
        if rest.is_empty() {
            return Err(());
        }
        let Some(constraint_version) = parse(rest) else {
            return Err(());
        };
        let order = version.compare(&constraint_version);
        let holds = match operator {
            ">=" => prerelease_check(&version, &constraint_version) && order.is_ge(),
            "<=" => prerelease_check(&version, &constraint_version) && order.is_le(),
            "!=" => order.is_ne(),
            ">" => prerelease_check(&version, &constraint_version) && order.is_gt(),
            "<" => prerelease_check(&version, &constraint_version) && order.is_lt(),
            "=" => order.is_eq(),
            _ => return Err(()),
        };
        if !holds {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::satisfies;

    #[test]
    fn compares_ordinary_tags() {
        assert_eq!(satisfies("v1.2.3", ">= 1.2.3"), Ok(true));
        assert_eq!(satisfies("1.2.3", "> 1.2.3"), Ok(false));
        assert_eq!(satisfies("1.10.0", "> 1.9.0"), Ok(true));
        assert_eq!(satisfies("1.0", "= 1.0.0"), Ok(true));
        assert_eq!(satisfies("v4.15.0", "> 4.14.1"), Ok(true));
        assert_eq!(satisfies("v4.14.1", "<= 4.14.1"), Ok(true));
    }

    #[test]
    fn combines_constraints_with_commas() {
        assert_eq!(satisfies("1.5.0", ">= 1.0.0, < 2.0.0"), Ok(true));
        assert_eq!(satisfies("2.0.0", ">= 1.0.0, < 2.0.0"), Ok(false));
        assert_eq!(satisfies("1.5.0", "!= 1.5.0"), Ok(false));
    }

    #[test]
    fn treats_unparseable_versions_as_not_matching() {
        assert_eq!(satisfies("nightly", ">= 1.0.0"), Ok(false));
        assert_eq!(
            satisfies("0123456789abcdef0123456789abcdef01234567", ">= 1.0.0"),
            Ok(false)
        );
    }

    #[test]
    fn rejects_malformed_constraints() {
        assert_eq!(satisfies("1.0.0", ""), Err(()));
        assert_eq!(satisfies("1.0.0", "1.0.0"), Err(()));
        assert_eq!(satisfies("1.0.0", ">= "), Err(()));
        assert_eq!(satisfies("1.0.0", "~> 1.0.0"), Err(()));
        assert_eq!(satisfies("1.0.0", ">= nope"), Err(()));
    }

    #[test]
    fn applies_hashicorp_prerelease_rule_to_range_constraints() {
        // A prerelease version does not satisfy a range constraint that has no
        // prerelease, even when numeric ordering would include it.
        assert_eq!(satisfies("1.0.0-rc.1", "< 1.0.0"), Ok(false));
        assert_eq!(satisfies("1.0.1-rc.1", ">= 1.0.0"), Ok(false));

        // A prerelease constraint may still match a non-prerelease version.
        assert_eq!(satisfies("1.0.0", "> 1.0.0-rc.1"), Ok(true));
        assert_eq!(satisfies("1.0.0", ">= 1.0.0-rc.1"), Ok(true));
        assert_eq!(satisfies("2.0.0", "> 1.0.0-rc.1"), Ok(true));

        // When both sides have prerelease data, the numeric base segments must
        // match exactly.
        assert_eq!(satisfies("1.0.0-rc.10", "> 1.0.0-rc.2"), Ok(true));
        assert_eq!(satisfies("1.0.0-rc.2", ">= 1.0.0-rc.1"), Ok(true));
        assert_eq!(satisfies("1.0.0-rc.1", "<= 1.0.0-rc.1"), Ok(true));
        assert_eq!(satisfies("1.0.1-rc.1", "> 1.0.0-rc.1"), Ok(false));
        assert_eq!(satisfies("2.0.0-rc.1", ">= 1.0.0-rc.1"), Ok(false));

        // Short versions are padded to three segments, but a fourth segment is
        // a different base.
        assert_eq!(satisfies("1.0-rc.1", ">= 1.0.0-rc.1"), Ok(true));
        assert_eq!(satisfies("1.0.0.0-rc.1", ">= 1.0.0-rc.1"), Ok(false));

        // Equality and inequality do not use the range rule.
        assert_eq!(satisfies("1.0.0-rc.1", "= 1.0.0"), Ok(false));
        assert_eq!(satisfies("1.0.0-rc.1", "!= 1.0.0"), Ok(true));
    }

    #[test]
    fn orders_prereleases_before_releases_when_the_base_matches() {
        assert_eq!(satisfies("1.0.0-alpha", "< 1.0.0-beta"), Ok(true));
        assert_eq!(satisfies("1.0.0-rc.2", "< 1.0.0"), Ok(false));
    }

    #[test]
    fn accepts_prereleases_without_a_hyphen() {
        assert_eq!(satisfies("1.2.0b3", "<= 1.2.0b3"), Ok(true));
        assert_eq!(satisfies("2.0.0a2", ">= 2.0.0a2, <= 2.0.0-b1"), Ok(true));
        assert_eq!(satisfies("1.2.0b3", "= 1.2.0b3"), Ok(true));
        assert_eq!(satisfies("1.2.0b3", "< 1.2.0"), Ok(false));
    }
}
