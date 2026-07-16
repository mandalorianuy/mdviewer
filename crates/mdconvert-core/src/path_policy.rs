use unicode_normalization::UnicodeNormalization;

/// Applies Windows basename semantics to one already-separated path component.
///
/// Windows reserves these names case-insensitively, even with an extension or
/// trailing dots/spaces. NFKC also folds the recognized superscript COM/LPT
/// digits into their ordinary numeric forms.
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
    let stem: String = stem.nfkc().flat_map(char::to_lowercase).collect();
    matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "clock$"
            | "conin$"
            | "conout$"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
            | "globalroot"
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
}
