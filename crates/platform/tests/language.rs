use harbormaster_platform::preferred_language;

#[test]
fn names_the_language_the_user_prefers_first() {
    let language = preferred_language().expect("macOS should name a preferred language");

    assert!(!language.is_empty() && language.is_ascii(), "{language:?}");
}
