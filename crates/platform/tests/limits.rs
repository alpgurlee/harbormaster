#![cfg(target_os = "macos")]

use std::mem::MaybeUninit;

fn soft_and_hard() -> (u64, u64) {
    let mut limit = MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: `limit` has room for one rlimit, which getrlimit fills on success.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) },
        0
    );
    // SAFETY: getrlimit succeeded, so it filled `limit`.
    let limit = unsafe { limit.assume_init() };
    (limit.rlim_cur, limit.rlim_max)
}

#[test]
fn raises_the_open_file_limit_launchd_gives_an_app() {
    let (_, hard) = soft_and_hard();
    let launchd = libc::rlimit {
        rlim_cur: hard.min(256),
        rlim_max: hard,
    };
    // SAFETY: a valid rlimit; lowering the soft limit is always allowed.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &launchd) }, 0);

    let raised = harbormaster_platform::allow_more_open_files().unwrap();

    assert_eq!(raised, hard.min(10_240));
    assert_eq!(soft_and_hard(), (raised, hard));
    assert_eq!(
        harbormaster_platform::allow_more_open_files().unwrap(),
        raised
    );
    assert_eq!(soft_and_hard(), (raised, hard));
}
