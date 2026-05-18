# EIA test fixtures

## `eia-fuel-type-sample.json`

**Provenance:** a *real* response from the EIA-930 v2 API
(`electricity/rto/fuel-type-data`), captured 2026-05-17, then **sanitized**
to strip EIA's request echo (which contains the API key) — only the
`response` object is retained. It is not synthetic.

**Query captured:**

- endpoint: `https://api.eia.gov/v2/electricity/rto/fuel-type-data/data/`
- `frequency=hourly`, `data[0]=value`
- `facets[respondent][]=CISO`, `facets[respondent][]=ERCO`
- `start=2026-05-12T00`, `end=2026-05-12T06`
- `sort=period asc`, `length=300`

**Contents:** 119 rows, respondents `CISO` and `ERCO`, hourly periods
`2026-05-12T00` … `2026-05-12T06`. Fuel codes present include the
standard set plus `GEO` (geothermal → folded into `OTH`) and `BAT`
(battery storage → excluded from the generation mix). `value` is a JSON
string in this API version.

**Regenerating:** re-run the query above with `EIA_API_KEY` from `.env`,
then keep only the `response` object (e.g. `python3 -c 'import json,sys;
json.dump({"response": json.load(open("raw.json"))["response"]},
open("eia-fuel-type-sample.json","w"), indent=2, sort_keys=True)'`).
Never commit the raw response — it embeds the API key in `request`.
