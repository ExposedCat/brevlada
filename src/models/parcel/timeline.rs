use super::{Parcel, Step};

pub struct Point {
    pub step: Step,
    pub confirmed: bool,
}

impl Parcel {
    pub fn timeline(&self) -> Vec<Point> {
        let mut points: Vec<_> = self
            .steps
            .iter()
            .cloned()
            .map(|step| Point {
                confirmed: !matches!(step.name.as_deref(), Some("Expected delivery")),
                step,
            })
            .collect();
        if points.is_empty() && self.completed() {
            points.push(Point {
                step: Step {
                    name: self.details["deliveryStatus"]
                        .as_str()
                        .map(super::state::label),
                    date: None,
                    location: None,
                },
                confirmed: true,
            });
        }
        if points.is_empty() || (points.len() == 1 && self.completed()) {
            points.insert(
                0,
                Point {
                    step: Step {
                        name: Some("Sent".into()),
                        date: None,
                        location: None,
                    },
                    confirmed: false,
                },
            );
        }
        if !self.completed()
            && !points
                .iter()
                .any(|point| point.step.name.as_deref() == Some("Expected delivery"))
        {
            points.push(Point {
                step: Step {
                    name: Some("Expected delivery".into()),
                    date: self
                        .arrival_until
                        .clone()
                        .or_else(|| self.arrival_from.clone()),
                    location: None,
                },
                confirmed: false,
            });
        }
        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_history_uses_unknown_sent_date_and_never_confirms_expected_delivery() {
        let mut parcel: Parcel = serde_json::from_value(serde_json::json!({
            "item": null, "arrival_from": null, "arrival_until": "2026-10-06T21:00:00Z",
            "carrier": null, "tracking_url": null, "order_url": null, "order_number": null,
            "steps": [], "details": {}
        }))
        .unwrap();
        let points = parcel.timeline();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].step.name.as_deref(), Some("Sent"));
        assert_eq!(points[0].step.date, None);
        assert_eq!(points[1].step.name.as_deref(), Some("Expected delivery"));
        assert_eq!(points[1].step.date, parcel.arrival_until);
        assert!(points.iter().all(|point| !point.confirmed));
        parcel.steps.push(Step {
            name: Some("Delivered".into()),
            date: None,
            location: None,
        });
        let points = parcel.timeline();
        assert_eq!(points.len(), 2);
        assert!(!points[0].confirmed);
        assert!(points[1].confirmed);
        assert_eq!(points[1].step.name.as_deref(), Some("Delivered"));
    }
}
