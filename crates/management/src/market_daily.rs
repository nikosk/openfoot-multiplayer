//! Source transfer day phases: returns/development before play; deferred
//! registration after closing-date payroll. Never run a bot-only auto buy option.
use super::*;
use chrono::Datelike;

impl Football {
    pub(crate) fn advance_market_before_matches(&mut self, today: NaiveDate) -> Result<(), String> {
        let Some(market) = &self.management.market else {
            return Ok(());
        };
        if market.last_processed == Some(today) {
            return Ok(());
        }
        if market.last_processed.is_some_and(|last| last > today) {
            return Err("Market date moved backwards".into());
        }
        self.refresh_market_projection()?;
        let loans: Vec<_> = self
            .management
            .social
            .as_ref()
            .unwrap()
            .source_players
            .values()
            .filter_map(|p| {
                p.active_loan
                    .as_ref()
                    .map(|loan| (p.id.clone(), loan.clone()))
            })
            .collect();
        for (id, loan) in loans {
            let start: NaiveDate = loan.start_date.parse().map_err(|_| "Invalid loan start")?;
            let end: NaiveDate = loan.end_date.parse().map_err(|_| "Invalid loan end")?;
            let days = (today - start).num_days().max(0);
            let final_report = end <= today;
            if final_report || (days > 0 && days % 30 == 0) {
                self.market_loan_report(&id, &loan, today, final_report)?;
            }
            if final_report {
                self.management.clear_market_roster_references(&id, None);
                let occupied: std::collections::BTreeSet<_> = self
                    .management
                    .social
                    .as_ref()
                    .unwrap()
                    .source_players
                    .values()
                    .filter(|p| {
                        p.id != id && self.management.players[&p.id].club_id == loan.parent_team_id
                    })
                    .filter_map(|p| p.jersey_number)
                    .collect();
                let source = self
                    .management
                    .social
                    .as_mut()
                    .unwrap()
                    .source_players
                    .get_mut(&id)
                    .unwrap();
                source.jersey_number = source
                    .jersey_number
                    .filter(|n| !occupied.contains(n))
                    .or_else(|| (1..=99).find(|n| !occupied.contains(n)));
                source.active_loan = None;
                source.team_id = Some(loan.parent_team_id.clone());
                source.loan_listed = false;
                source.movement_history.push(PlayerMovementEntry {
                    date: today.to_string(),
                    kind: PlayerMovementKind::LoanReturn,
                    from_team_id: Some(loan.loan_team_id.clone()),
                    from_team_name: Some(self.management.clubs[&loan.loan_team_id].name.clone()),
                    to_team_id: Some(loan.parent_team_id.clone()),
                    to_team_name: Some(self.management.clubs[&loan.parent_team_id].name.clone()),
                    fee: None,
                    loan_end_date: Some(loan.end_date),
                });
                self.management.players.get_mut(&id).unwrap().club_id = loan.parent_team_id.clone();
                for club in [&loan.loan_team_id, &loan.parent_team_id] {
                    let revision = self.management.club_revisions.get_mut(club).unwrap();
                    *revision = revision.checked_add(1).ok_or("Club revision overflow")?;
                }
                let revision = self.management.player_revisions.get_mut(&id).unwrap();
                *revision = revision.checked_add(1).ok_or("Player revision overflow")?;
                self.management.contract_transferred(&id);
            }
        }
        self.management.market.as_mut().unwrap().last_processed = Some(today);
        Ok(())
    }
    fn market_loan_report(
        &mut self,
        id: &str,
        loan: &ActiveLoan,
        today: NaiveDate,
        final_report: bool,
    ) -> Result<(), String> {
        if self.management.market.as_ref().unwrap().reports.contains(&(
            id.into(),
            today,
            final_report,
        )) {
            return Ok(());
        }
        let source = self
            .management
            .social
            .as_mut()
            .unwrap()
            .source_players
            .get_mut(id)
            .unwrap();
        let delta = |current: u32, reported: u32| {
            if current >= reported {
                current - reported
            } else {
                current
            }
        };
        let minutes = delta(
            source.stats.minutes_played,
            loan.development_reported_minutes,
        );
        let appearances = delta(
            source.stats.appearances,
            loan.development_reported_appearances,
        );
        let rep = self.management.career.as_ref().unwrap().reputations[&loan.loan_team_id];
        let growth_room = source.potential.saturating_sub(source.ovr);
        let mut cycles: u8 = if minutes >= 900 || appearances >= 8 {
            2
        } else if minutes >= 180
            || appearances >= 2
            || ((minutes > 0 || appearances > 0) && rep >= 650)
        {
            1
        } else {
            0
        };
        if self
            .management
            .availability
            .as_ref()
            .and_then(|a| a.get(id))
            .is_some_and(|a| a.injury.is_some())
            || source.injury.is_some()
        {
            cycles = cycles.saturating_sub(1)
        }
        cycles = cycles.min(growth_room).min(2);
        let before = source.ovr;
        let attrs = &mut source.attributes;
        use domain::player::Position as P;
        let fields = match source.natural_position.to_group_position() {
            P::Goalkeeper => [&mut attrs.handling, &mut attrs.reflexes, &mut attrs.aerial],
            P::Defender => [
                &mut attrs.defending,
                &mut attrs.tackling,
                &mut attrs.positioning,
            ],
            P::Midfielder => [&mut attrs.passing, &mut attrs.vision, &mut attrs.decisions],
            _ => [
                &mut attrs.shooting,
                &mut attrs.dribbling,
                &mut attrs.positioning,
            ],
        };
        let mut gains = 0_u8;
        for field in fields {
            let gain = cycles.min(99_u8.saturating_sub(*field));
            *field += gain;
            gains += gain;
        }
        if gains > 0 {
            let engine = self
                .attributes
                .get_mut(id)
                .ok_or("Missing loan attributes")?;
            let value = serde_json::to_value(&source.attributes).map_err(|e| e.to_string())?;
            let mut engine_value = serde_json::to_value(&*engine).map_err(|e| e.to_string())?;
            for (key, value) in value.as_object().ok_or("Invalid attribute shape")? {
                engine_value[key] = value.clone();
            }
            *engine = serde_json::from_value(engine_value).map_err(|e| e.to_string())?;
            let natural: crate::training::Position = serde_json::from_value(
                serde_json::to_value(&source.natural_position).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let assigned: crate::training::Position = serde_json::from_value(
                serde_json::to_value(&source.position).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let position = if natural.is_legacy() {
                assigned
            } else {
                natural
            };
            source.ovr = crate::training::ovr_for_position(engine, position).round() as u8;
            source.potential = source.potential.max(source.ovr);
            source.traits =
                domain::player::compute_traits(&source.attributes, &source.natural_position);
            let year = source
                .date_of_birth
                .split('-')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(today.year() as u32);
            if (today.year() as u32).saturating_sub(year) <= 20
                && source.potential >= 90
                && source.potential.saturating_sub(source.ovr) >= 14
            {
                source.traits.push(domain::player::PlayerTrait::Wonderkid);
            }
            engine.ovr = source.ovr;
            engine.traits = source.traits.iter().map(|t| format!("{t:?}")).collect();
            if let Some(training) = &mut self.management.training {
                training
                    .players
                    .get_mut(id)
                    .ok_or("Missing training player")?
                    .potential = source.potential;
            }
        }
        let current_minutes = source.stats.minutes_played;
        let current_appearances = source.stats.appearances;
        let active = source.active_loan.as_mut().unwrap();
        active.development_reported_minutes = current_minutes;
        active.development_reported_appearances = current_appearances;
        let name = source.full_name.clone();
        let after = source.ovr;
        let recipient = self
            .management
            .managers
            .values()
            .find(|m| m.club_id == loan.parent_team_id)
            .map(|m| m.id.clone());
        if let Some(recipient) = recipient {
            use domain::message::*;
            let prefix = if final_report {
                "loan_return_report"
            } else {
                "loan_development"
            };
            let message = InboxMessage::new(
                format!("{prefix}_{id}_{today}"),
                "Loan development report".into(),
                format!("{name}: overall {before} → {after}; attribute gains {gains}."),
                self.management.clubs[&loan.loan_team_id].name.clone(),
                today.to_string(),
            )
            .with_category(MessageCategory::Transfer)
            .with_context(MessageContext {
                player_id: Some(id.into()),
                team_id: Some(loan.parent_team_id.clone()),
                ..Default::default()
            });
            self.management
                .social
                .as_mut()
                .unwrap()
                .inbox
                .deliver(&recipient, message)
                .map_err(|e| format!("{e:?}"))?;
        }
        self.management
            .market
            .as_mut()
            .unwrap()
            .reports
            .push((id.into(), today, final_report));
        Ok(())
    }
    pub(crate) fn advance_market_registrations(&mut self, today: NaiveDate) -> Result<(), String> {
        let Some(market) = &self.management.market else {
            return Ok(());
        };
        if market.last_registrations == Some(today) {
            return Ok(());
        }
        // Payroll already advanced the clock. Restore it on every return;
        // whole-day Football staging owns rollback of this late source phase.
        let next_date = self.management.career_date().ok_or("Missing career date")?;
        self.management.career.as_mut().unwrap().today = today;
        let result = (|| -> Result<(), String> {
            self.refresh_market_projection()?;
            self.management
                .expire_market_offers()
                .map_err(|e| format!("{e:?}"))?;
            let open =
                rules::registration_date(today, &self.management.market.as_ref().unwrap().window)
                    .is_ok_and(|d| d == today);
            if open {
                let due: Vec<_> = self
                    .management
                    .market
                    .as_ref()
                    .unwrap()
                    .offers
                    .values()
                    .filter(|o| {
                        o.status == Status::PendingRegistration
                            && o.registration_date.is_some_and(|date| date <= today)
                    })
                    .cloned()
                    .collect();
                for mut offer in due {
                    let valid = self.management.market_dependencies(&offer, false).is_ok();
                    if valid {
                        let mut staged = self.management.clone();
                        match staged.complete_market_move(&offer, false) {
                            Ok(()) => {
                                self.management = staged;
                                offer.status = Status::Completed;
                                offer.registration_date = Some(today)
                            }
                            Err(Error::Overflow) => {
                                return Err("Market registration arithmetic overflow".into());
                            }
                            Err(_) => offer.status = Status::Withdrawn,
                        }
                    } else {
                        offer.status = Status::Withdrawn
                    }
                    self.management
                        .market
                        .as_mut()
                        .unwrap()
                        .offers
                        .insert(offer.id, offer.clone());
                    self.management
                        .sync_market_offer(offer.id)
                        .map_err(|e| format!("{e:?}"))?;
                }
            }
            let market = self.management.market.as_mut().unwrap();
            market.previews.clear();
            market.last_registrations = Some(today);
            Ok(())
        })();
        self.management.career.as_mut().unwrap().today = next_date;
        result
    }
}
