//! `Oryxis::handle_net_tools`: the network tools panel's dispatch arms.
//!
//! Every probe runs as a `Task::perform` on the tokio runtime, so a
//! traceroute walking twenty hops never blocks a frame. What comes back
//! is matched against the panel's own run counter before it is shown:
//! a request already in flight cannot be recalled, so "cancel" and
//! "run something else" both work by making the old answer irrelevant
//! rather than by trying to stop it.

use iced::Task;

use crate::app::{Message, NetToolsMessage, Oryxis};

impl Oryxis {
    pub(crate) fn handle_net_tools(&mut self, message: NetToolsMessage) -> Task<Message> {
        match message {
            NetToolsMessage::Select(tool) => {
                if self.net_tools.tool != tool {
                    self.net_tools.tool = tool;
                    // The cards answered the previous tool's question;
                    // leaving them under a new selector would read as
                    // this tool's output.
                    self.net_tools.reset();
                }
                Task::none()
            }
            NetToolsMessage::Target(value) => {
                self.net_tools.target = value;
                self.net_tools.error = None;
                Task::none()
            }
            NetToolsMessage::Ports(value) => {
                self.net_tools.ports = value;
                self.net_tools.error = None;
                Task::none()
            }
            NetToolsMessage::Run => self.net_tools_run(),
            NetToolsMessage::Finished(seq, result) => {
                if !self.net_tools.is_current(seq) {
                    // A superseded run. Its answer is about a target the
                    // user has moved on from.
                    return Task::none();
                }
                self.net_tools.running = None;
                match result {
                    Ok(cards) => self.net_tools.cards = cards,
                    Err(e) => self.net_tools.error = Some(e),
                }
                Task::none()
            }
            NetToolsMessage::Cancel => {
                self.net_tools.running = None;
                Task::none()
            }
            NetToolsMessage::CopyCard(idx) => {
                let Some(card) = self.net_tools.cards.get(idx) else {
                    return Task::none();
                };
                let text = card.raw.clone();
                Task::batch([
                    crate::dispatch_global::write_clipboard_text(text),
                    self.show_toast(crate::i18n::t("net_copied").to_string()),
                ])
            }
            NetToolsMessage::ResultHovered(idx) => {
                self.hover.net_tools_card = Some(idx);
                Task::none()
            }
            NetToolsMessage::ResultUnhovered(idx) => {
                // Crossing from one card to the next publishes the
                // arriving card's enter before the departing card's
                // exit, so the clear has to name the card it is leaving
                // (the card-action convention in CLAUDE.md).
                self.hover.leave_net_tools_card(idx);
                Task::none()
            }
        }
    }

    /// Start a run, unless one is already in flight or the panel has
    /// nothing to run against.
    fn net_tools_run(&mut self) -> Task<Message> {
        if self.net_tools.running.is_some() {
            return Task::none();
        }
        if self.net_tools.target.trim().is_empty() {
            self.net_tools.error = Some(crate::i18n::t("net_err_no_target").to_string());
            return Task::none();
        }
        let tool = self.net_tools.tool;
        let target = self.net_tools.target.clone();
        let ports = self.net_tools.ports.clone();
        let seq = self.net_tools.begin();
        Task::perform(crate::net_tools::run(tool, target, ports), move |result| {
            Message::NetTools(NetToolsMessage::Finished(seq, result))
        })
    }
}
