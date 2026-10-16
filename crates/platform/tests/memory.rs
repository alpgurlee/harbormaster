use harbormaster_platform::memory;

#[test]
fn reports_used_and_total_memory_of_this_machine() {
    let memory = memory().expect("memory figures should be readable");

    assert!(memory.total > 512 * 1024 * 1024, "{memory:?}");
    assert!(memory.used > 0 && memory.used <= memory.total, "{memory:?}");
}
