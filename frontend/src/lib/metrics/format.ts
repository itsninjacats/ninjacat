const SI = ['', 'k', 'M', 'G', 'T', 'P'];
const two = new Intl.NumberFormat('en', { maximumFractionDigits: 2, useGrouping: false });
const three = new Intl.NumberFormat('en', { maximumSignificantDigits: 3, useGrouping: false });

// Short enough to sit beside a chart: 11 771 153 846 is 11.77G. English on
// purpose: other locales spell the suffix out ("11,8 mld") and get clipped.
export function formatValue(v: number): string {
	const a = Math.abs(v);
	if (a !== 0 && (a < 0.001 || a >= 1e15)) return v.toExponential(1);
	// Below one, two decimals would round 0.004 s to 0.
	if (a < 1) return three.format(v);
	let e = a < 1000 ? 0 : Math.floor(Math.log10(a) / 3);
	// 999 999 rounds to 1000k; it is 1M.
	if (Number(two.format(a / 1000 ** e)) >= 1000) e++;
	return two.format(v / 1000 ** e) + SI[e];
}
