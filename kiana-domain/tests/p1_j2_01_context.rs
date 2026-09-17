use kiana_domain::{prompt_hash, render_prompt, PromptAuthority, PromptSection};

#[test]
fn context_sections_render_with_provenance() {
    let sections = vec![
        PromptSection {
            name: "later".to_owned(),
            order: 200,
            text: "second".to_owned(),
            source: "memory:later".to_owned(),
            authority: PromptAuthority::Context,
        },
        PromptSection {
            name: "first".to_owned(),
            order: 100,
            text: "first".to_owned(),
            source: "artifact:first".to_owned(),
            authority: PromptAuthority::Context,
        },
    ];
    assert_eq!(render_prompt(&sections), "first\n\nsecond");
    let provenance = sections
        .iter()
        .map(PromptSection::provenance)
        .collect::<Vec<_>>();
    assert_eq!(provenance[0]["source"], "memory:later");
    assert_eq!(provenance[0]["prompt_hash"], prompt_hash("second"));
    assert_eq!(provenance[1]["order"], 100);
    assert!(provenance.iter().all(|item| item["authority"] == "context"));
}

#[test]
fn prompt_render_order_is_deterministic_for_equal_orders() {
    let sections = vec![
        PromptSection {
            name: "z".to_owned(),
            order: 100,
            text: "z".to_owned(),
            source: "source:z".to_owned(),
            authority: PromptAuthority::Context,
        },
        PromptSection {
            name: "a".to_owned(),
            order: 100,
            text: "a".to_owned(),
            source: "source:a".to_owned(),
            authority: PromptAuthority::Context,
        },
    ];
    assert_eq!(render_prompt(&sections), "a\n\nz");
}
