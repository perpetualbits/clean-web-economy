# DAPR Simulation

Use `sample_usage.csv` to simulate weighted allocations.

Formula per user:
D_total = Σ(minutes_i * price_i * region_factor_i)
W_i = (minutes_i * price_i * region_factor_i) / D_total
R_i = tier_fee * W_i
