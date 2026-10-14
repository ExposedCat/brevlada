#[derive(Clone, Copy, Default)]
pub(super) struct Options {
    pub(super) keep_checking: bool,
    pub(super) start_at_login: bool,
}

impl Options {
    pub(super) fn needs_background(self) -> bool {
        self.keep_checking || self.start_at_login
    }

    pub(super) fn stays_alive(self, login_session: bool) -> bool {
        self.keep_checking || (login_session && self.start_at_login)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_startup_and_window_closing_have_independent_lifetimes() {
        for keep_checking in [false, true] {
            for start_at_login in [false, true] {
                let options = Options {
                    keep_checking,
                    start_at_login,
                };
                assert_eq!(options.stays_alive(false), keep_checking);
                assert_eq!(options.stays_alive(true), keep_checking || start_at_login);
                assert_eq!(options.needs_background(), keep_checking || start_at_login);
            }
        }
    }
}
