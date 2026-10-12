// SPDX-FileCopyrightText: 2026 Igor Santos
// SPDX-License-Identifier: Apache-2.0

//! Placeholder crate. Real code lands in a later change.

/// Returns the crate name.
pub fn name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_not_empty() {
        assert!(!name().is_empty());
    }
}
