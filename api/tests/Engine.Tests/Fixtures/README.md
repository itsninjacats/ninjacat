# Reference outputs

`stl.json` and `sarima.json` are what statsmodels computes on fixed series;
the engine's STL and airline SARIMA are tested against them. They come from
the scripts beside them:

```bash
python3 -m venv /tmp/ref && /tmp/ref/bin/pip install statsmodels numpy
/tmp/ref/bin/python stl.py > stl.json        # statsmodels 0.15
/tmp/ref/bin/python sarima.py > sarima.json
```

statsmodels is the reference because it is the widely used open
implementation of both: its STL follows Cleveland et al.'s Fortran, its
SARIMAX is the standard state-space ARIMA.
