/// Applies Windows basename semantics to one already-separated path component.
///
/// Windows reserves these names case-insensitively, even with an extension or
/// trailing dots/spaces. The aliases are literal ASCII names, except that
/// Windows also recognizes superscript 1, 2, and 3 as COM/LPT suffixes.
pub fn is_windows_reserved_component(component: &str) -> bool {
    let component = component.trim_end_matches([' ', '.']);
    if component.is_empty() {
        return false;
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches([' ', '.']);
    [
        "con",
        "prn",
        "aux",
        "nul",
        "clock$",
        "conin$",
        "conout$",
        "globalroot",
    ]
    .iter()
    .any(|alias| stem.eq_ignore_ascii_case(alias))
        || is_reserved_numbered_port(stem)
}

fn is_reserved_numbered_port(stem: &str) -> bool {
    let bytes = stem.as_bytes();
    if bytes.len() <= 3 || !bytes[..3].iter().all(u8::is_ascii) {
        return false;
    }

    let (prefix, suffix) = stem.split_at(3);
    (prefix.eq_ignore_ascii_case("com") || prefix.eq_ignore_ascii_case("lpt"))
        && matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_components_include_console_alias_variants() {
        for component in [
            "CONIN$",
            "conout$.md",
            "CoNiN$.txt... ",
            "cOnOuT$ .log",
            "COM¹.txt",
            "LPT².log",
        ] {
            assert!(is_windows_reserved_component(component), "{component:?}");
        }
    }

    #[test]
    fn compatibility_characters_do_not_become_reserved_aliases() {
        for component in ["ＣＯＮ.json", "COM①.json", "ＣＯＭ1.json"] {
            assert!(!is_windows_reserved_component(component), "{component:?}");
        }
    }
}
