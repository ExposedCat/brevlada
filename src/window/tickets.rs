use super::*;
use models::ticket::Reservation;

impl State {
    pub(super) fn ticket_widgets(
        &self,
        messages: &[(bool, String, Message)],
    ) -> Vec<Vec<gtk::Widget>> {
        let mut widgets = vec![Vec::new(); messages.len()];
        let mut reservations: Vec<(usize, String, Reservation)> = Vec::new();
        for (index, (_, _, message)) in messages
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, (_, _, message))| !message.is_draft)
        {
            let sender = models::senders::key(message);
            for reservation in &message.tickets {
                let provider = reservation
                    .provider
                    .as_ref()
                    .map(|provider| provider.to_lowercase())
                    .unwrap_or_else(|| sender.clone());
                if let Some((_, _, older)) = reservations.iter_mut().find(|(_, from, older)| {
                    *from == provider && older.same_reservation(reservation)
                }) {
                    older.merge(reservation);
                } else {
                    reservations.push((index, provider, reservation.clone()));
                }
            }
        }
        for (index, _, reservation) in reservations {
            widgets[index].push(ui::ticket::card(&reservation).upcast());
        }
        widgets
    }
}
