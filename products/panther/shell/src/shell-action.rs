// @file products/panther/shell/src/shell-action.rs
// @description Defines the neutral action the shell reports to the window.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Neutral shell action.
//!
//! The shell reports one of these actions to the window and names no product
//! type. A tab action carries a slot index, never a `TabId`; `SubmitAddress`
//! carries the raw typed text. The window is the sole translator: it maps a slot
//! index to a `TabId` and parses the address text through the product core (D3,
//! D6). `SubmitAddress` carries an owned `String`, so the action is not `Copy`.

/// Neutral action the shell reports from pointer or key input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellAction {
    ActivateTab(usize),
    NewTab,
    CloseTab(usize),
    SubmitAddress(String),
}
