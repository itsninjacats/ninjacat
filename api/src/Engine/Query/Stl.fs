/// STL: Seasonal-Trend decomposition based on Loess.
///
///   R.B. Cleveland, W.S. Cleveland, J.E. McRae and I. Terpenning (1990),
///   "STL: A Seasonal-Trend Decomposition Procedure Based on Loess",
///   Journal of Official Statistics 6(1), 3–73.
///
/// Ported line by line from statsmodels' statsmodels/tsa/stl/_stl.pyx
/// (statsmodels 0.15), which is itself a port of the NETLIB Fortran. The
/// Fortran names are kept (ess, est, ss, fts, rwts) so the three can be read
/// side by side; indices stay 1-based where the original's are.
///
/// _stl.pyx's own header reads "(c) 2019 Kevin Sheppard / License: NCSA/BSD-3
/// Clause / Based on NETLIB STL code". statsmodels' licence (LICENSE.txt,
/// statsmodels 0.15.0), which asks that this notice be retained, verbatim:
///
///   Copyright (C) 2006, Jonathan E. Taylor
///   All rights reserved.
///
///   Copyright (c) 2006-2008 Scipy Developers.
///   All rights reserved.
///
///   Copyright (c) 2009-2018 statsmodels Developers.
///   All rights reserved.
///
///
///   Redistribution and use in source and binary forms, with or without
///   modification, are permitted provided that the following conditions are met:
///
///     a. Redistributions of source code must retain the above copyright notice,
///        this list of conditions and the following disclaimer.
///     b. Redistributions in binary form must reproduce the above copyright
///        notice, this list of conditions and the following disclaimer in the
///        documentation and/or other materials provided with the distribution.
///     c. Neither the name of statsmodels nor the names of its contributors
///        may be used to endorse or promote products derived from this software
///        without specific prior written permission.
///
///
///   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
///   AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
///   IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
///   ARE DISCLAIMED. IN NO EVENT SHALL STATSMODELS OR CONTRIBUTORS BE LIABLE FOR
///   ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
///   DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
///   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
///   CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
///   LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
///   OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
///   DAMAGE.
///
/// Tested against statsmodels on a fixed series (tests/Engine.Tests/Fixtures).
module NinjaCat.Api.Engine.Query.Stl

open System

type Settings =
    {
        /// np: points per season.
        Period: int
        /// ns: seasonal smoother length, odd, ≥ 3.
        Seasonal: int
        /// nt: trend smoother length, odd, > period.
        Trend: int
        /// nl: low-pass filter length, odd, > period.
        LowPass: int
        SeasonalDeg: int
        TrendDeg: int
        LowPassDeg: int
        SeasonalJump: int
        TrendJump: int
        LowPassJump: int
        /// ni
        InnerIter: int
        /// no: robustness iterations; 0 for a non-robust fit.
        OuterIter: int
    }

/// statsmodels' defaults for a period, robust: seasonal 7; trend the next odd
/// number ≥ 1.5·period / (1 − 1.5/7); low-pass the next odd number > period;
/// degrees 1; jumps 1; 2 inner and 15 robustness iterations.
let robustDefaults (period: int) : Settings =
    let seasonal = 7
    let odd (x: int) = if x % 2 = 0 then x + 1 else x

    { Period = period
      Seasonal = seasonal
      Trend = odd (int (Math.Ceiling(1.5 * float period / (1.0 - 1.5 / float seasonal))))
      LowPass = odd (period + 1)
      SeasonalDeg = 1
      TrendDeg = 1
      LowPassDeg = 1
      SeasonalJump = 1
      TrendJump = 1
      LowPassJump = 1
      InnerIter = 2
      OuterIter = 15 }

type Decomposition =
    { Trend: float[]
      Seasonal: float[]
      Remainder: float[]
      /// Robustness weights: 1 for a point fully trusted, down to 0.
      Weights: float[] }

/// Loess estimate at xs (1-based) from y[nleft-1 .. nright-1], with weights
/// written to w. NaN when every weight is zero.
let private est (y: float[]) (n: int) (len: int) (ideg: int) (xs: int) (nleft: int) (nright: int) (w: float[]) (userw: bool) (rw: float[]) : float =
    let rng = float n - 1.0
    let mutable h = float (max (xs - nleft) (nright - xs))

    if len > n then
        h <- h + Math.Floor(float (len - n) / 2.0)

    let h9 = 0.999 * h
    let h1 = 0.001 * h
    let mutable a = 0.0

    for j in nleft - 1 .. nright - 1 do
        w[j] <- 0.0
        let r = abs (float (j + 1 - xs))

        if r <= h9 then
            if r <= h1 then
                w[j] <- 1.0
            else
                let q = r / h
                let t = 1.0 - q * q * q
                w[j] <- t * t * t

            if userw then
                w[j] <- w[j] * rw[j]

            a <- a + w[j]

    if a <= 0.0 then
        nan
    else
        for j in nleft - 1 .. nright - 1 do
            w[j] <- w[j] / a

        if h > 0.0 && ideg > 0 then
            let mutable a = 0.0

            for j in nleft - 1 .. nright - 1 do
                a <- a + w[j] * float (j + 1)

            let mutable b = float xs - a
            let mutable c = 0.0

            for j in nleft - 1 .. nright - 1 do
                let d = float (j + 1) - a
                c <- c + w[j] * d * d

            if sqrt c > 0.001 * rng then
                b <- b / c

                for j in nleft - 1 .. nright - 1 do
                    w[j] <- w[j] * (b * (float (j + 1) - a) + 1.0)

        let mutable ys = 0.0

        for j in nleft - 1 .. nright - 1 do
            ys <- ys + w[j] * y[j]

        ys

/// Loess smoothing of y into ys[off ..], every njump points, interpolated
/// between.
let private ess (y: float[]) (n: int) (len: int) (ideg: int) (njump: int) (userw: bool) (rw: float[]) (ys: float[]) (off: int) (res: float[]) =
    if n < 2 then
        ys[off] <- y[0]
    else
        let newnj = min njump (n - 1)
        let mutable nleft = 0
        let mutable nright = 0

        let estimate i =
            let v = est y n len ideg (i + 1) nleft nright res userw rw
            ys[off + i] <- if Double.IsNaN v then y[i] else v

        if len >= n then
            nleft <- 1
            nright <- n
            let mutable i = 0

            while i < n do
                estimate i
                i <- i + newnj
        elif newnj = 1 then
            let nsh = (len + 2) / 2
            nleft <- 1
            nright <- len

            for i in 0 .. n - 1 do
                if i + 1 > nsh && nright <> n then
                    nleft <- nleft + 1
                    nright <- nright + 1

                estimate i
        else
            let nsh = (len + 1) / 2
            let mutable i = 0

            while i < n do
                if i + 1 < nsh then
                    nleft <- 1
                    nright <- len
                elif i + 1 >= n - nsh + 1 then
                    nleft <- n - len + 1
                    nright <- n
                else
                    nleft <- i + 1 - nsh + 1
                    nright <- len + i + 1 - nsh

                estimate i
                i <- i + newnj

        if newnj <> 1 then
            let mutable i = 0

            while i < n - newnj do
                let delta = (ys[off + i + newnj] - ys[off + i]) / float newnj

                for j in i .. i + newnj - 1 do
                    ys[off + j] <- ys[off + i] + delta * float ((j + 1) - (i + 1))

                i <- i + newnj

            let k = ((n - 1) / newnj) * newnj + 1

            if k <> n then
                estimate (n - 1)

                if k <> n - 1 then
                    let delta = (ys[off + n - 1] - ys[off + k - 1]) / float (n - k)

                    for j in k .. n - 1 do
                        ys[off + j] <- ys[off + k - 1] + delta * float ((j + 1) - k)

/// Moving average of length len over x[0 .. n-1] into ave.
let private ma (x: float[]) (n: int) (len: int) (ave: float[]) =
    let newn = n - len + 1
    let flen = float len
    let mutable v = 0.0

    for i in 0 .. len - 1 do
        v <- v + x[i]

    ave[0] <- v / flen
    let mutable k = len
    let mutable m = 0

    for j in 1 .. newn - 1 do
        v <- v + x[k] - x[m]
        ave[j] <- v / flen
        k <- k + 1
        m <- m + 1

/// Decomposes evenly spaced values without gaps.
let decompose (s: Settings) (y: float[]) : Decomposition =
    let n = y.Length
    let np = s.Period
    let trend = Array.zeroCreate n
    let season = Array.zeroCreate n
    let rw = Array.create n 1.0
    let work = Array.init 5 (fun _ -> Array.zeroCreate<float> (n + 2 * np))
    let mutable userw = false

    // ss: seasonal smoothing of each cycle-subseries of work[0] into work[1],
    // one period extended at each end.
    let ss () =
        let yy, seasonOut, work1, work2, work3, work4 = work[0], work[1], work[2], work[3], work[4], season

        for j in 0 .. np - 1 do
            let k = (n - (j + 1)) / np + 1

            for i in 0 .. k - 1 do
                work1[i] <- yy[i * np + j]

            if userw then
                for i in 0 .. k - 1 do
                    work3[i] <- rw[i * np + j]

            ess work1 k s.Seasonal s.SeasonalDeg s.SeasonalJump userw work3 work2 1 work4
            let nright = min s.Seasonal k
            let first = est work1 k s.Seasonal s.SeasonalDeg 0 1 nright work4 userw work3
            work2[0] <- if Double.IsNaN first then work2[1] else first
            let nleft = max 1 (k - s.Seasonal + 1)
            let last = est work1 k s.Seasonal s.SeasonalDeg (k + 1) nleft k work4 userw work3
            work2[k + 1] <- if Double.IsNaN last then work2[k] else last

            for m in 0 .. k + 1 do
                seasonOut[m * np + j] <- work2[m]

    // fts: the low-pass filter's three moving averages, work[1] → work[2].
    let fts () =
        let len = n + 2 * np
        ma work[1] len np work[2]
        ma work[2] (len - np + 1) np work[0]
        ma work[0] (len - 2 * np + 2) 3 work[2]

    let onestp () =
        for _ in 1 .. s.InnerIter do
            for i in 0 .. n - 1 do
                work[0][i] <- y[i] - trend[i]

            ss ()
            fts ()
            ess work[2] n s.LowPass s.LowPassDeg s.LowPassJump false work[3] work[0] 0 work[4]

            for i in 0 .. n - 1 do
                season[i] <- work[1][np + i] - work[0][i]
                work[0][i] <- y[i] - season[i]

            ess work[0] n s.Trend s.TrendDeg s.TrendJump userw rw trend 0 work[2]

    // rwts: bisquare robustness weights from |y − fit|, fit in work[0].
    let rwts () =
        for i in 0 .. n - 1 do
            rw[i] <- abs (y[i] - work[0][i])

        let sorted = Array.sort rw
        let mid0 = n / 2
        let mid1 = n - mid0 - 1
        let cmad = 3.0 * (sorted[mid0] + sorted[mid1])

        if cmad = 0.0 then
            Array.fill rw 0 n 1.0
        else
            let c9 = 0.999 * cmad
            let c1 = 0.001 * cmad

            for i in 0 .. n - 1 do
                rw[i] <-
                    if rw[i] <= c1 then 1.0
                    elif rw[i] <= c9 then let q = rw[i] / cmad in let t = 1.0 - q * q in t * t
                    else 0.0

    let mutable k = 0
    let mutable go = true

    while go do
        onestp ()
        k <- k + 1

        if k > s.OuterIter then
            go <- false
        else
            for i in 0 .. n - 1 do
                work[0][i] <- trend[i] + season[i]

            rwts ()
            userw <- true

    { Trend = trend
      Seasonal = season
      Remainder = Array.init n (fun i -> y[i] - season[i] - trend[i])
      Weights = rw }
