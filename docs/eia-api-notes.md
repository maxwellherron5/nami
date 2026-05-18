# EIA-930 v2 API notes

> Living document. Records what the real API returns and the assumptions
> `nami` makes parsing it. Update in the same change that changes
> `nami_carbon_eia::api`.

## Endpoint

```
GET https://api.eia.gov/v2/electricity/rto/fuel-type-data/data/
    ?frequency=hourly
    &data[0]=value
    &facets[respondent][]=CISO  (one per region queried)
    &start=YYYY-MM-DDTHH&end=YYYY-MM-DDTHH
    &sort[0][column]=period&sort[0][direction]=asc
    &length=...
    &api_key=...        # from EIA_API_KEY env / .env, never committed
```

Free key: <https://www.eia.gov/opendata/register.php>. ~1–2 h reporting
lag. Phase 0 uses `fuel-type-data` only (generation by fuel type); the
`region-data` and `interchange-data` endpoints are not needed for
average-intensity derivation.

## Response shape (confirmed from a real capture, 2026-05-17)

```jsonc
{
  "response": {
    "data": [
      { "period": "2026-05-12T00", "respondent": "CISO",
        "respondent-name": "...", "fueltype": "NG", "type-name": "...",
        "value": "4886", "value-units": "megawatthours" }
    ],
    "dateFormat": "...", "frequency": "hourly", "total": "...",
    "description": "..."
  },
  "request": { ... },          // ECHOES THE API KEY — never persist this
  "apiVersion": "...", "ExcelAddInVersion": "..."
}
```

- **`value` is a JSON string** (e.g. `"4886"`, `"-55"`, `"0"`), not a
  number, in this API version. The parser also accepts numbers and
  `null` defensively.
- **`period` is `YYYY-MM-DDTHH` in UTC.** EIA-930 RTO hourly series are
  UTC; CLAUDE.md mandates treating them as such. Parsed to a UTC
  `OffsetDateTime` at the top of the hour.
- The `request` object echoes the API key — only the `response` object is
  ever saved to a fixture (see `crates/nami-carbon-eia/tests/fixtures/README.md`).

## Respondent code mapping

EIA balancing-authority codes differ from `nami`'s `Region`:

| EIA respondent | `Region` |
|---|---|
| `CISO` | CAISO |
| `ERCO` | ERCOT |
| `MISO` | MISO |
| `PJM`  | PJM |
| `NYIS` | NYISO |
| `ISNE` | ISONE |
| `SWPP` | SPP |

An **unrecognized respondent is a hard error** (`Error::Malformed`): we
only ever query the seven Phase-0 regions, so an unexpected respondent
indicates a query/scope bug, not benign extra data.

## Fuel-type handling (methodology)

The real API returned more granular codes than CLAUDE.md's 9-category
list. Decisions (see also `docs/methodology.md`):

- **Standard codes** `COL, NG, NUC, OIL, WAT, SUN, WND, OTH, UNK` map
  1:1 to `FuelType`.
- **`GEO` (geothermal) → `OTH`.** Consistent with CLAUDE.md's definition
  of `OTH` as "biomass, geothermal, etc." When `GEO` and `OTH` both
  appear in the same region-hour, their MWh values are **summed**.
- **`BAT` (battery) and `PS` (pumped storage) are excluded.** These are
  storage, not primary generation; their values can be negative
  (charging) and they carry no intrinsic emission factor (emissions
  belong to the charging source). Counting them in `Σ generation` would
  bias average intensity, so storage rows are dropped from the mix
  entirely.
- **Unrecognized codes → `UNK` + surfaced note.** A genuinely unknown
  code maps to `FuelType::Unk` and records a note on the affected
  `FuelMixHour` (so the assumption is visible and `nami` keeps working if
  EIA adds a code), rather than hard-failing.

## Missing / bad values

- `null`, absent, or empty `value` → the fuel was not reported that hour;
  the row is **skipped** (absence is not a fabricated zero).
- A present but unparseable or non-finite `value` → skipped **and** a
  note is recorded on the `FuelMixHour`.
- Negative values for non-storage fuels are kept as-is; how the
  derivation (item 8) treats them is a separate, documented decision.

## Fetching & cache refresh (item 13)

`nami refresh --region <R> [--weeks N] [--cache PATH] [--egrid PATH]`
fetches **one** region's recent history and merges it into the local
historical cache; other regions in the cache are preserved. `--weeks`
defaults to `DEFAULT_FORECAST_WEEKS` (8). `EIA_API_KEY` is required (a
missing key is a hard error, not a silent fallback).

- **Pagination.** EIA v2 caps a response at 5000 rows. `nami` pages with
  `offset`/`length`, accumulating raw `response.data` rows *before*
  parsing, then runs `parse_fuel_type_data` once over the combined
  document. Concatenating before parsing matters: a page boundary can
  fall mid-hour, and parsing pages independently would split one
  region-hour into two partial mixes (and two same-timestamp
  observations, which the cache's strict validation rejects). Stops on a
  short/empty page or when `offset >= total`; a `MAX_PAGES` cap prevents
  an infinite loop on a bad `total`.
- **`total`** is a JSON string in this API version (a number is also
  accepted defensively).
- **Window.** `[now − N weeks, now]`, both truncated to the top of the
  UTC hour.
- **Gaps.** An hour with no positive generation (`DerivationFailed`) is
  counted and **skipped**, never written as a fabricated zero. An
  unexpected respondent/region in the response is a hard error
  (`Malformed`) — the request facet should make it impossible.
- **Cache safety.** A missing cache is created; an existing-but-unusable
  cache is **refused**, not overwritten.
- The `request` echo (which contains the API key) is never parsed or
  persisted — only `response` is read; error bodies are truncated.
