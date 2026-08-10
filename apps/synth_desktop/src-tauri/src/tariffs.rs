//! Versioned provider tariff catalog.
//!
//! One backend home for every price the app is allowed to reason about — the
//! renderer renders what it is given and never hard-codes a rate. An entry
//! only exists where the *device* is the payer (direct provider keys). Synth
//! Cloud usage is billed by the backend and local Laguna has no provider
//! charge, so neither gets a tariff: their cost stays `None` rather than a
//! plausible invention.
//!
//! Estimates produced here are exactly that — `cost_source` records them as
//! `tariff_estimate`, and any provider-reported settled charge always
//! replaces them (see `storage::usage_records`).

/// A provider price card effective from a given instant. Newer entries for
/// the same (provider, model) supersede older ones at their effective time,
/// so historic requests keep being priced by the tariff of their day.
#[derive(Clone, Copy, Debug)]
pub struct Tariff {
    pub provider: &'static str,
    pub model_id: &'static str,
    /// Unix milliseconds UTC of the first instant this tariff applies.
    pub effective_from_ms: i64,
    pub input_usd_per_m: f64,
    pub output_usd_per_m: f64,
    /// Cached input reads. `None` means the provider publishes no cache-read
    /// discount and cached tokens are priced at the plain input rate.
    pub cached_input_usd_per_m: Option<f64>,
    /// Cache writes. `None` means no published write premium: write tokens
    /// are priced at the plain input rate they are already part of.
    pub cache_write_usd_per_m: Option<f64>,
}

/// 2026-08-01T00:00:00Z — the day the current OpenRouter cards were entered.
const AUG_2026_MS: i64 = 1_785_542_400_000;

const CATALOG: &[Tariff] = &[
    Tariff {
        provider: "openrouter",
        model_id: "openai/gpt-5.6-luna",
        effective_from_ms: AUG_2026_MS,
        input_usd_per_m: 0.20,
        output_usd_per_m: 1.20,
        cached_input_usd_per_m: Some(0.02),
        cache_write_usd_per_m: Some(0.25),
    },
    Tariff {
        provider: "openrouter",
        model_id: "poolside/laguna-s-2.1",
        effective_from_ms: AUG_2026_MS,
        input_usd_per_m: 0.10,
        output_usd_per_m: 0.20,
        cached_input_usd_per_m: None,
        cache_write_usd_per_m: None,
    },
];

/// One catalog entry as the renderer receives it — the same numbers the
/// estimator prices with, so Settings can never drift from billing.
#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffCard {
    pub provider: &'static str,
    pub model_id: &'static str,
    pub input_usd_per_m: f64,
    pub output_usd_per_m: f64,
    pub cached_input_usd_per_m: Option<f64>,
    pub cache_write_usd_per_m: Option<f64>,
}

/// The tariffs in force at `at_ms`, one card per (provider, model).
pub fn cards_in_force(at_ms: i64) -> Vec<TariffCard> {
    CATALOG
        .iter()
        .filter_map(|entry| tariff_for(entry.provider, entry.model_id, at_ms))
        .map(|tariff| TariffCard {
            provider: tariff.provider,
            model_id: tariff.model_id,
            input_usd_per_m: tariff.input_usd_per_m,
            output_usd_per_m: tariff.output_usd_per_m,
            cached_input_usd_per_m: tariff.cached_input_usd_per_m,
            cache_write_usd_per_m: tariff.cache_write_usd_per_m,
        })
        .fold(Vec::new(), |mut cards, card| {
            // `tariff_for` already resolved superseded entries; folding by key
            // keeps one card even when the catalog holds historic revisions.
            if !cards
                .iter()
                .any(|kept: &TariffCard| kept.provider == card.provider && kept.model_id == card.model_id)
            {
                cards.push(card);
            }
            cards
        })
}

/// The tariff in force for a request completed at `at_ms`, if any.
pub fn tariff_for(provider: &str, model_id: &str, at_ms: i64) -> Option<&'static Tariff> {
    CATALOG
        .iter()
        .filter(|tariff| {
            tariff.provider.eq_ignore_ascii_case(provider)
                && tariff.model_id.eq_ignore_ascii_case(model_id)
                && tariff.effective_from_ms <= at_ms
        })
        .max_by_key(|tariff| tariff.effective_from_ms)
}

/// Token counts as reported by the provider; `None` means unreported, and is
/// treated as zero only for pricing (never persisted as zero).
#[derive(Clone, Copy, Debug, Default)]
pub struct BillableTokens {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

/// Tariff estimate for one request, or `None` when no tariff covers it or no
/// token counts exist to price.
///
/// Semantics: cached reads are clamped to the reported input, cache writes to
/// the remainder — provider counters include both inside `input_tokens` — and
/// output is priced as reported (reasoning is already inside the provider's
/// output count, so it is never added again).
pub fn estimate_cost_usd(
    provider: &str,
    model_id: &str,
    at_ms: i64,
    tokens: BillableTokens,
) -> Option<f64> {
    let tariff = tariff_for(provider, model_id, at_ms)?;
    if tokens.input_tokens.is_none() && tokens.output_tokens.is_none() {
        return None;
    }
    let input = tokens.input_tokens.unwrap_or(0).max(0);
    let cached = tokens.cached_input_tokens.unwrap_or(0).clamp(0, input);
    let writes = tokens.cache_write_tokens.unwrap_or(0).clamp(0, input - cached);
    let base = input - cached - writes;
    let output = tokens.output_tokens.unwrap_or(0).max(0);

    let cached_rate = tariff.cached_input_usd_per_m.unwrap_or(tariff.input_usd_per_m);
    let write_rate = tariff.cache_write_usd_per_m.unwrap_or(tariff.input_usd_per_m);
    let per_m = |tokens: i64, rate: f64| tokens as f64 * rate / 1_000_000.0;
    Some(
        per_m(base, tariff.input_usd_per_m)
            + per_m(cached, cached_rate)
            + per_m(writes, write_rate)
            + per_m(output, tariff.output_usd_per_m),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luna_estimate_uses_all_four_tariff_dimensions() {
        // 1M of each bucket makes the expected figure the card itself:
        // base input $0.20 + cached reads $0.02 + cache writes $0.25 + output $1.20.
        let tokens = BillableTokens {
            input_tokens: Some(3_000_000),
            cached_input_tokens: Some(1_000_000),
            cache_write_tokens: Some(1_000_000),
            output_tokens: Some(1_000_000),
        };
        let cost = estimate_cost_usd("openrouter", "openai/gpt-5.6-luna", AUG_2026_MS, tokens)
            .expect("Luna is in the catalog");
        assert!((cost - (0.20 + 0.02 + 0.25 + 1.20)).abs() < 1e-9, "{cost}");
    }

    #[test]
    fn cached_and_write_counts_never_exceed_reported_input() {
        let tokens = BillableTokens {
            input_tokens: Some(100),
            cached_input_tokens: Some(1_000_000),
            cache_write_tokens: Some(1_000_000),
            output_tokens: None,
        };
        // Everything is clamped inside the 100 input tokens, priced at the
        // cached rate ($0.02/M) once the clamp assigns them all to cache reads.
        let cost = estimate_cost_usd("openrouter", "openai/gpt-5.6-luna", AUG_2026_MS, tokens)
            .expect("estimable");
        assert!(cost > 0.0 && cost < 100.0 * 0.25 / 1_000_000.0, "{cost}");
    }

    #[test]
    fn unpriced_providers_and_empty_usage_yield_no_estimate() {
        assert!(estimate_cost_usd(
            "local-laguna",
            "poolside/Laguna-XS-2.1-NVFP4-mlx",
            AUG_2026_MS,
            BillableTokens {
                input_tokens: Some(10),
                output_tokens: Some(10),
                ..Default::default()
            }
        )
        .is_none());
        assert!(estimate_cost_usd(
            "synth-cloud",
            "openrouter/poolside/laguna-s-2.1",
            AUG_2026_MS,
            BillableTokens {
                input_tokens: Some(10),
                output_tokens: Some(10),
                ..Default::default()
            }
        )
        .is_none());
        assert!(estimate_cost_usd(
            "openrouter",
            "openai/gpt-5.6-luna",
            AUG_2026_MS,
            BillableTokens::default()
        )
        .is_none());
    }

    #[test]
    fn the_served_catalog_carries_the_estimators_own_numbers_once_per_model() {
        let cards = cards_in_force(AUG_2026_MS);
        assert_eq!(cards.len(), 2);
        let luna = cards
            .iter()
            .find(|card| card.model_id == "openai/gpt-5.6-luna")
            .unwrap();
        assert_eq!(luna.provider, "openrouter");
        assert_eq!(luna.input_usd_per_m, 0.20);
        assert_eq!(luna.output_usd_per_m, 1.20);
        assert_eq!(luna.cached_input_usd_per_m, Some(0.02));
        assert_eq!(luna.cache_write_usd_per_m, Some(0.25));
        let laguna = cards
            .iter()
            .find(|card| card.model_id == "poolside/laguna-s-2.1")
            .unwrap();
        assert_eq!(laguna.cached_input_usd_per_m, None);
        // Before any card is in force there is nothing to serve — the UI
        // shows nothing rather than a stale or invented rate.
        assert!(cards_in_force(AUG_2026_MS - 1).is_empty());
    }

    #[test]
    fn requests_before_a_tariffs_effective_date_are_not_priced_by_it() {
        assert!(tariff_for("openrouter", "openai/gpt-5.6-luna", AUG_2026_MS - 1).is_none());
        assert!(tariff_for("openrouter", "openai/gpt-5.6-luna", AUG_2026_MS).is_some());
    }
}
