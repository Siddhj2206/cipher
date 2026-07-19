#![cfg(test)]

use crate::glossary::GlossaryTerm;
use crate::translate::TranslateOptions;

pub fn glossary_term(term: &str, og_term: Option<&str>, definition: &str) -> GlossaryTerm {
    GlossaryTerm {
        term: term.to_string(),
        og_term: og_term.map(str::to_string),
        definition: definition.to_string(),
        notes: None,
    }
}

pub fn translate_options(rerun: Option<crate::RerunMode>) -> TranslateOptions {
    TranslateOptions {
        profile: None,
        repair_profile: None,
        glossary_profile: None,
        overwrite: false,
        fail_fast: false,
        rerun,
        dry_run: false,
    }
}

pub fn smart_glossary(hero_definition: &str) -> Vec<GlossaryTerm> {
    vec![
        glossary_term("Hero", Some("勇者"), hero_definition),
        glossary_term("Mage", Some("魔導士"), "Mage definition"),
        glossary_term("Holy Sword", Some("聖剣"), "Sword definition"),
        glossary_term("Royal Castle", Some("王城"), "Castle definition"),
        glossary_term("Dragon King", Some("竜王"), "Dragon definition"),
    ]
}

pub fn smart_text() -> &'static str {
    "勇者は魔導士と聖剣を手に王城で竜王と戦った。"
}
