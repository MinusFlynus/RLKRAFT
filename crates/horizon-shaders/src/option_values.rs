//! Scalar shader-pack options from the adjacent `<pack>.txt` file.
//! Applied while reading source for compilation, never during a frame.
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Values(BTreeMap<String, String>);

impl Values {
    pub(crate) fn parse(text: &str) -> Self {
        let mut values = BTreeMap::new();
        for line in text.lines().take(2048) {
            let Some((name, value)) = line.trim().split_once('=') else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            // Only identifiers and scalar literals: never insert source code.
            let identifier = !name.is_empty()
                && name.len() <= 128
                && name.bytes().enumerate().all(|(i, b)| {
                    b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())
                });
            let scalar = matches!(value, "true" | "false")
                || (!value.is_empty()
                    && value.len() <= 32
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b".+-eEfF".contains(&b))
                    && value
                        .strip_suffix('f')
                        .or_else(|| value.strip_suffix('F'))
                        .unwrap_or(value)
                        .parse::<f64>()
                        .is_ok_and(f64::is_finite));
            if identifier && scalar && values.len() < 256 {
                values.insert(name.to_owned(), value.to_owned());
            }
        }
        Self(values)
    }

    pub(crate) fn apply(&self, text: String) -> String {
        if self.0.is_empty() {
            return text;
        }
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let define = trimmed
                .strip_prefix("#define")
                .or_else(|| trimmed.strip_prefix("//#define"))
                .filter(|rest| rest.starts_with(char::is_whitespace));
            let mut replacement = None;
            if let Some(rest) = define {
                let name = rest.split_whitespace().next().unwrap_or("");
                if let Some(value) = self.0.get(name) {
                    replacement = Some(match value.as_str() {
                        "false" => format!("{indent}//#define {name}"),
                        "true" => format!("{indent}#define {name}"),
                        _ => format!("{indent}#define {name} {value}"),
                    });
                }
            } else if let Some(rest) = trimmed.strip_prefix("const ")
                && let Some((left, _)) = rest.split_once('=')
            {
                let words: Vec<_> = left.split_whitespace().collect();
                if let [ty @ ("float" | "int" | "bool"), name] = words.as_slice()
                    && let Some(value) = self.0.get(*name)
                {
                    replacement = Some(format!("{indent}const {ty} {name} = {value};"));
                }
            }
            if let Some(replacement) = replacement {
                out.push_str(&replacement);
                if line.ends_with('\n') {
                    out.push('\n');
                }
            } else {
                out.push_str(line);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_numeric_switch_and_distance_options_without_modifying_the_input() {
        let values = Values::parse("QUALITY=1\nSHADOWS=false\nCLOUDS=true\nshadowDistance=128.0");
        let source = "#version 330\n#define QUALITY 3 // [1 2 3]\n#define SHADOWS // toggle\n//#define CLOUDS\nconst float shadowDistance = 256.0;\n#include \"shared.glsl\"\n";
        let result = values.apply(source.to_owned());
        assert_eq!(
            result,
            "#version 330\n#define QUALITY 1\n//#define SHADOWS\n#define CLOUDS\nconst float shadowDistance = 128.0;\n#include \"shared.glsl\"\n"
        );
        assert!(source.contains("QUALITY 3"));
    }

    #[test]
    fn rejects_source_injection_and_preserves_unrelated_defines_and_line_counts() {
        let values = Values::parse("GOOD=2\nBAD=1;discard\nFUNCTION(x)=2\nINFINITY=inf\nNAN=NaN\n");
        let source = "#define GOODNESS 4\n#define BAD 1\n#define GOOD 0\n#define FUNCTION(x) x\n";
        let result = values.apply(source.to_owned());
        assert_eq!(result.lines().count(), source.lines().count());
        assert!(result.contains("#define GOODNESS 4"));
        assert!(result.contains("#define BAD 1"));
        assert!(result.contains("#define GOOD 2"));
        assert!(result.contains("#define FUNCTION(x) x"));
        assert_eq!(Values::default().apply(source.to_owned()), source);
    }
}
