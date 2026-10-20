import json, math, numpy as np
from statsmodels.tsa.seasonal import STL
period = 24
n = period * 6
# Deterministic "noise" so both sides see the same numbers.
y = [10 + 0.05 * t + 3 * math.sin(2 * math.pi * t / period) + 0.5 * math.sin(t * 1.7) * math.cos(t * 0.3) for t in range(n)]
y[100] += 15  # one spike, for the robust weights
res = STL(np.array(y), period=period, robust=True).fit()
m = STL(np.array(y), period=period, robust=True)
print(json.dumps({"period": period, "y": y, "trend": list(res.trend), "seasonal": list(res.seasonal), "resid": list(res.resid),
  "params": {"seasonal": m.config["seasonal"], "trend": m.config["trend"], "low_pass": m.config["low_pass"],
             "seasonal_deg": m.config["seasonal_deg"], "trend_deg": m.config["trend_deg"], "low_pass_deg": m.config["low_pass_deg"],
             "seasonal_jump": m.config["seasonal_jump"], "trend_jump": m.config["trend_jump"], "low_pass_jump": m.config["low_pass_jump"]}}))
