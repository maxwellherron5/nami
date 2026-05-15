# Public data sources

> **Status:** living document. Add a row before depending on a new source.

## Required properties

A `nami` Phase 0 data source must be:

- Free to access (no commercial contract).
- Publicly documented.
- Auditable: third parties can verify our numbers from the same inputs.
- Stable enough for a CLI user to depend on across sessions.
- Explicit about freshness, lag, and granularity.
- Compatible licence for redistribution of derived numbers.

A source that fails any of these is not Phase 0 material, however
convenient it might be.

## Phase 0 sources

### EIA-930 (Hourly Electric Grid Monitor)

- **What:** hourly U.S. balancing-authority data — demand, day-ahead
  demand forecast, net generation, generation by fuel type, interchange.
- **Why:** the only free, comprehensive, hourly U.S. fuel-mix source.
- **Where:** `https://api.eia.gov/v2/electricity/rto/...` (key required;
  free registration at `https://www.eia.gov/opendata/register.php`).
  Bulk CSVs at `https://www.eia.gov/electricity/gridmonitor/sixMonthFiles/`.
- **Granularity:** hourly, UTC timestamps (always — local zones in the
  data don't always match physical location).
- **Lag:** ~1–2 hours typical.
- **Limitations:** sum of fuel-type generation doesn't always equal
  reported total; smaller BAs are noisier; `OTH` sometimes hides
  confidential generators.
- **Phase 0 BAs:** CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP.

### EPA eGRID

- **What:** static emission factors per fuel type at multiple geographic
  granularities (national, subregion, state, plant).
- **Why:** publicly maintained, methodologically transparent, widely
  cited reference.
- **Granularity:** subregion is the right level for `nami`.
- **Limitations:** annual averages, not real-time; refreshed every
  12–18 months; geographic mapping to BAs is approximate.
- **Storage:** committed `data/egrid-factors.toml`, version-pinned to a
  specific eGRID release.

## Phase 1 candidates

Promising but not Phase 0:

- **Open Grid Emissions** — pre-derived hourly emissions; reduces our
  derivation work.
- **CAISO public feeds** — renewable forecast, generation data.
- **ERCOT public feeds** — generation, renewable forecast.
- **PJM Data Miner** — market and grid data.
- **SPP public feeds** — load and renewable forecast.
- **NYISO / ISONE / MISO** public feeds where licensing permits.
- **UK Carbon Intensity API** — free, generous, well-documented (UK only).
- **ENTSO-E Transparency Platform** — European grid data (registration required).

Phase 1 sources may introduce uneven regional capabilities; that is
acceptable provided each is explicit.

## Phase 2+ candidates

Deferred unless explicitly approved:

- WattTime (commercial; free tier limited to CAISO_NORTH).
- Electricity Maps commercial API.
- Cloud-vendor carbon APIs.
- Any source with paid access, restrictive redistribution, or
  credentialed-only data beyond free registration.

## What we will not use

- Anything requiring NDA or paid licence in Phase 0.
- Anything whose terms prohibit redistributing derived numbers.
- Scraped data without an explicit terms-of-use review.
- Data with no documented methodology.
