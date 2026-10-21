# Committee Economics Model

This model makes the relationship between table rake, committee costs, and player expected value explicit. It is a planning model, not a forecast or a promise of returns. Replace the illustrative inputs with measured network and product data before using it for a production decision.

## Model boundary

The model is evaluated per table-hour. A table-hour is one active table with a stable player count for one hour. The model includes:

- gross pot volume and the configured rake rate;
- the per-pot rake cap;
- the portion of rake allocated to the committee;
- fixed and variable infrastructure costs; and
- the expected value retained by players after rake.

It does not include token price changes, treasury appreciation, taxes, support payroll, marketing, chargebacks, or capital expenditure. Those items should be added as separate budget lines instead of being hidden in the assumptions below.

## Inputs and formulas

| Symbol | Meaning | Illustrative value |
| --- | --- | ---: |
| `P` | Average completed pots per table-hour | 90 |
| `B` | Average pot size | 12 XLM |
| `r` | Rake rate | 2.5% |
| `C` | Rake cap per pot | 0.50 XLM |
| `a` | Committee share of collected rake | 30% |
| `N` | Active tables | 25 |
| `F` | Fixed infrastructure cost per month | 2,000 XLM |
| `V` | Variable infrastructure cost per table-hour | 0.08 XLM |
| `H` | Operating table-hours per month | 720 |

For a pot, the uncapped rake is `B × r` and the collected rake is:

```text
pot_rake = min(B × r, C)
```

The monthly operating volume is:

```text
monthly_table_hours = N × H
monthly_pots = monthly_table_hours × P
monthly_gross_volume = monthly_pots × B
monthly_rake = monthly_pots × pot_rake
monthly_committee_revenue = monthly_rake × a
monthly_variable_cost = monthly_table_hours × V
monthly_net = monthly_committee_revenue - F - monthly_variable_cost
```

Player expected value from the table, before any game edge, is reduced by the rake paid by the table:

```text
player_rake_rate = pot_rake / B
player_net_volume = monthly_gross_volume - monthly_rake
```

The model should be recalculated with the observed pot-size distribution. A single average pot can overstate player cost when many pots hit the cap.

## Illustrative scenarios

The following scenarios hold `r`, `C`, `a`, `F`, and `V` constant and vary activity and average pot size. Values are monthly totals for the stated 25-table network and 720 operating hours per table.

| Scenario | Avg pot | Pots/table-hour | Gross volume | Rake collected | Committee revenue | Net after costs | Player rake rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Conservative | 8 XLM | 45 | 6,480,000 XLM | 162,000 XLM | 48,600 XLM | -3,400 XLM | 2.50% |
| Base | 12 XLM | 90 | 19,440,000 XLM | 486,000 XLM | 145,800 XLM | 122,360 XLM | 2.50% |
| Growth | 18 XLM | 135 | 43,740,000 XLM | 1,093,500 XLM | 328,050 XLM | 304,610 XLM | 2.50% |
| Cap-heavy | 40 XLM | 90 | 64,800,000 XLM | 648,000 XLM | 194,400 XLM | 170,960 XLM | 1.25% |

The cap-heavy row demonstrates why gross volume alone is not enough to estimate revenue: the 0.50 XLM cap reduces the effective player rake rate for larger pots and also limits the collected rake per pot.

## Sensitivity: activity and average pot size

The table below shows monthly net committee revenue after `F` and variable costs. It uses the same 25-table network, 30% committee share, 2.5% rake, and 0.50 XLM cap.

| Pots/table-hour \ Avg pot | 8 XLM | 12 XLM | 18 XLM | 40 XLM |
| --- | ---: | ---: | ---: | ---: |
| 45 | -3,400 XLM | 35,480 XLM | 53,720 XLM | 56,000 XLM |
| 90 | 34,720 XLM | 122,360 XLM | 186,080 XLM | 170,960 XLM |
| 135 | 72,840 XLM | 209,240 XLM | 318,440 XLM | 285,920 XLM |

Break-even table-hours are calculated from:

```text
break_even_table_hours = F / (P × pot_rake × a - V)
```

Using the base inputs, the committee requires approximately 10.5 table-hours per month to cover the fixed and variable costs in this simplified model. This low figure is a consequence of the illustrative volume and should not be treated as a launch target until the inputs are measured.

## Player EV guardrails

Revenue is sustainable only if players continue to receive acceptable value. The launch dashboard should report these guardrails alongside committee revenue:

1. Effective rake rate by pot-size bucket, including the share of pots that hit the cap.
2. Average and p95 time from table join to completed hand.
3. Net player volume after rake, separated by stake level and game type.
4. Active tables, completed pots, and unique players over the same reporting window.

The committee should review a configuration change when the effective rake rate rises above the published target, when net player volume declines for two consecutive reporting windows, or when infrastructure cost per table-hour exceeds the variable-cost assumption by 20%.

## Measurement plan

Before using this model for governance, collect a rolling seven-day sample containing table-hours, completed pots, pot sizes, rake charged, capped pots, active tables, and infrastructure invoices. Recompute the model daily, publish the assumptions with the result, and retain the raw aggregates so the committee can reproduce each reported figure.

## Limitations

- XLM-denominated costs and revenue are sensitive to network and market conditions.
- Average values hide skew from high-volume tables and capped pots.
- Player EV is not the same as player profit; game outcomes and skill are outside this model.
- The model does not recommend a rake change. Any change should be reviewed against player-rake guardrails and announced before activation.
