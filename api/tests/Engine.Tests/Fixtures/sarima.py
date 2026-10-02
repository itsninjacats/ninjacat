import json, numpy as np, warnings
from statsmodels.tsa.statespace.sarimax import SARIMAX
warnings.filterwarnings("ignore")
s, theta, Theta, n = 24, -0.4, -0.6, 24 * 20
rng = np.random.default_rng(42)
e = rng.normal(0, 1.0, n + s + 2)
# (1-B)(1-B^s) y_t = (1 + θB)(1 + ΘB^s) e_t, built step by step from zero history.
y = np.zeros(n + s + 2)
for t in range(s + 2, n + s + 2):
    w = e[t] + theta * e[t-1] + Theta * e[t-s] + theta * Theta * e[t-s-1]
    y[t] = y[t-1] + y[t-s] - y[t-s-1] + w
y = 100 + y[s + 2:]
m = SARIMAX(y, order=(0, 1, 1), seasonal_order=(0, 1, 1, s)).fit(disp=False)
f = m.get_forecast(steps=s)
print(json.dumps({"s": s, "y": list(y), "true": {"theta": theta, "Theta": Theta},
  "fit": {"theta": float(m.params[0]), "Theta": float(m.params[1]), "sigma2": float(m.params[2])},
  "forecast": {"mean": list(f.predicted_mean), "se": list(f.se_mean)}}))
