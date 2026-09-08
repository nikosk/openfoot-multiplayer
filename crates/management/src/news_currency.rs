//! Pinned OpenFoot Manager 64677fee. Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use serde::Serialize;

pub const DEFAULT_CURRENCY_CODE: &str = "EUR";

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CurrencyDefinition {
    pub code: &'static str,
    pub symbol: &'static str,
    pub exchange_rate: f64,
}

const SUPPORTED_CURRENCIES: [CurrencyDefinition; 3] = [
    CurrencyDefinition {
        code: "EUR",
        symbol: "€",
        exchange_rate: 1.0,
    },
    CurrencyDefinition {
        code: "GBP",
        symbol: "£",
        exchange_rate: 0.86,
    },
    CurrencyDefinition {
        code: "USD",
        symbol: "$",
        exchange_rate: 1.08,
    },
];

pub fn supported_currencies() -> Vec<CurrencyDefinition> {
    SUPPORTED_CURRENCIES.to_vec()
}

pub fn normalize_currency_code(code: &str) -> Option<&'static str> {
    match code.trim().to_ascii_uppercase().as_str() {
        "EUR" => Some("EUR"),
        "GBP" => Some("GBP"),
        "USD" => Some("USD"),
        _ => None,
    }
}

pub fn currency_definition(code: &str) -> Option<CurrencyDefinition> {
    let normalized = normalize_currency_code(code)?;
    SUPPORTED_CURRENCIES
        .iter()
        .copied()
        .find(|currency| currency.code == normalized)
}

pub fn convert_amount(amount: i64, code: &str) -> Option<i64> {
    let rate = currency_definition(code)?.exchange_rate;
    Some((amount as f64 * rate).round() as i64)
}

fn convert_unsigned_amount(amount: u64, code: &str) -> Option<u64> {
    let rate = currency_definition(code)?.exchange_rate;
    Some(((amount as f64) * rate).round().clamp(0.0, u64::MAX as f64) as u64)
}

pub fn format_compact_number(amount: u64, code: &str) -> Option<String> {
    let converted = convert_unsigned_amount(amount, code)?;

    if converted >= 1_000_000 {
        Some(format!("{:.1}M", converted as f64 / 1_000_000.0))
    } else if converted >= 1_000 {
        Some(format!("{}K", converted / 1_000))
    } else {
        Some(converted.to_string())
    }
}

pub fn format_compact_money(amount: u64, code: &str) -> Option<String> {
    let currency = currency_definition(code)?;
    Some(format!(
        "{}{}",
        currency.symbol,
        format_compact_number(amount, code)?
    ))
}

pub fn default_currency_symbol() -> &'static str {
    currency_definition(DEFAULT_CURRENCY_CODE)
        .map(|currency| currency.symbol)
        .unwrap_or("€")
}
