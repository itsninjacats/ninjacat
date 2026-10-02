// The functions the Σ menu offers: the ones the query engine supports, in
// Datadog's own families. `args` are what comes after the query, pre-filled
// with a working default and editable in the chip.

/** A function applied to a query or formula row, e.g. `anomalies` with `'basic', 2`. */
export type FunctionCall = { name: string; args: string[] };

export type FunctionSpec = {
	name: string;
	/** Default arguments after the query, as they are written in the formula. */
	args: string[];
	/** What each argument is, shown in the chip; one per entry in `args`. */
	argNames?: string[];
	/** One line on what the function does, shown in the Σ menu and on the chip. */
	description: string;
	/**
	 * `modifier`: appended to the query itself (`.rollup(sum, 60)`), so only a
	 * query row can take it. `outer`: must be a formula's outermost function.
	 */
	kind?: 'modifier' | 'outer';
};

export type FunctionFamily = { family: string; functions: FunctionSpec[] };

const ewma = (n: number): FunctionSpec => ({
	name: `ewma_${n}`,
	args: [],
	description: `Exponentially weighted moving average over about ${n} points.`
});
const median = (n: number): FunctionSpec => ({
	name: `median_${n}`,
	args: [],
	description: `Rolling median of the last ${n} points; removes spikes.`
});
const rollingavg = (n: number): FunctionSpec => ({
	name: `rollingavg_${n}`,
	args: [],
	description: `Rolling mean of the last ${n} points.`
});

export const FUNCTION_FAMILIES: FunctionFamily[] = [
	{
		family: 'Arithmetic',
		functions: [
			{ name: 'abs', args: [], description: 'Absolute value of each point.' },
			{ name: 'log2', args: [], description: 'Base-2 logarithm of each point.' },
			{ name: 'log10', args: [], description: 'Base-10 logarithm of each point.' },
			{ name: 'cumsum', args: [], description: 'Running total over the visible window.' },
			{
				name: 'integral',
				args: [],
				description: 'Running sum of value change × seconds between points.'
			},
			{
				name: 'round',
				args: ['2'],
				argNames: ['decimals'],
				description: 'Rounds each point to the given number of decimals.'
			},
			{ name: 'ceil', args: [], description: 'Rounds each point up.' },
			{ name: 'floor', args: [], description: 'Rounds each point down.' }
		]
	},
	{
		family: 'Rate',
		functions: [
			{ name: 'per_second', args: [], description: 'Rate of change per second.' },
			{ name: 'per_minute', args: [], description: 'Rate of change per minute.' },
			{ name: 'per_hour', args: [], description: 'Rate of change per hour.' },
			{ name: 'diff', args: [], description: 'Difference from the previous point.' },
			{
				name: 'monotonic_diff',
				args: [],
				description: 'Difference from the previous point, ignoring counter resets.'
			},
			{
				name: 'derivative',
				args: [],
				description: 'Difference from the previous point, per second.'
			},
			{
				name: 'throughput',
				args: [],
				description: 'Each point divided by the bucket width in seconds.'
			}
		]
	},
	{
		family: 'Smoothing',
		functions: [
			{
				name: 'autosmooth',
				args: [],
				description: 'Smooths noise while keeping the shape; picks the window itself.'
			},
			ewma(3),
			ewma(5),
			ewma(10),
			ewma(20),
			median(3),
			median(5),
			median(7),
			median(9),
			rollingavg(5),
			rollingavg(13),
			rollingavg(21),
			rollingavg(29)
		]
	},
	{
		family: 'Rollup',
		functions: [
			{
				name: 'rollup',
				args: ['avg', '60'],
				argNames: ['method', 'seconds'],
				description:
					'How points in one bucket combine (avg, sum, min, max, count), and the bucket width.',
				kind: 'modifier'
			},
			{
				name: 'as_count',
				args: [],
				description: 'Counts and rates as the number of events per bucket.',
				kind: 'modifier'
			},
			{
				name: 'as_rate',
				args: [],
				description: 'Counts and rates as events per second.',
				kind: 'modifier'
			}
		]
	},
	{
		family: 'Interpolation',
		functions: [
			{
				name: 'fill',
				args: ['linear', '300'],
				argNames: ['method', 'seconds'],
				description: 'Fills gaps (null, zero, last, linear) up to the given seconds.',
				kind: 'modifier'
			},
			{ name: 'default_zero', args: [], description: 'Empty buckets become 0.' }
		]
	},
	{
		family: 'Rank',
		functions: [
			{
				name: 'top',
				args: ['10', "'mean'", "'desc'"],
				argNames: ['limit', 'rank by', 'order'],
				description: 'Keeps the top or bottom N series (limit 5, 10, 25, 50, 100).'
			}
		]
	},
	{
		family: 'Count',
		functions: [
			{
				name: 'count_nonzero',
				args: [],
				description: 'How many series are non-zero at each point.'
			},
			{
				name: 'count_not_null',
				args: [],
				description: 'How many series have a value at each point.'
			}
		]
	},
	{
		family: 'Exclusion',
		functions: [
			{ name: 'exclude_null', args: [], description: 'Drops series whose group has an N/A tag.' },
			{
				name: 'clamp_min',
				args: ['0'],
				argNames: ['minimum'],
				description: 'Raises points below the threshold to it.'
			},
			{
				name: 'clamp_max',
				args: ['100'],
				argNames: ['maximum'],
				description: 'Lowers points above the threshold to it.'
			},
			{
				name: 'cutoff_min',
				args: ['0'],
				argNames: ['minimum'],
				description: 'Removes points below the threshold.'
			},
			{
				name: 'cutoff_max',
				args: ['100'],
				argNames: ['maximum'],
				description: 'Removes points above the threshold.'
			}
		]
	},
	{
		family: 'Timeshift',
		functions: [
			{ name: 'hour_before', args: [], description: 'The same series one hour earlier.' },
			{ name: 'day_before', args: [], description: 'The same series one day earlier.' },
			{ name: 'week_before', args: [], description: 'The same series one week earlier.' },
			{
				name: 'month_before',
				args: [],
				description: 'The same series one calendar month earlier.'
			},
			{
				name: 'timeshift',
				args: ['-3600'],
				argNames: ['seconds'],
				description: 'The same series shifted by seconds (negative: the past).'
			},
			{
				name: 'calendar_shift',
				args: ["'-1d'"],
				argNames: ['shift'],
				description: "Shifted by calendar days, weeks or months: '-1d', '-2w', '-1mo'."
			}
		]
	},
	{
		family: 'Regression',
		functions: [
			{
				name: 'trend_line',
				args: [],
				description: 'Least-squares straight line through the points.'
			},
			{
				name: 'robust_trend',
				args: [],
				description: 'Straight line that is not pulled by outliers (Huber loss).'
			},
			{
				name: 'piecewise_constant',
				args: [],
				description: 'Flat segments that jump where the level changes.'
			}
		]
	},
	{
		family: 'Algorithms',
		functions: [
			{
				name: 'anomalies',
				args: ["'basic'", '2'],
				argNames: ['algorithm', 'bounds'],
				description:
					"Band of expected values: 'basic', 'agile' or 'robust'; bounds in standard deviations.",
				kind: 'outer'
			},
			{
				name: 'forecast',
				args: ["'linear'", '1'],
				argNames: ['algorithm', 'deviations'],
				description: "Predicts past the window: 'linear' or 'seasonal', with a band.",
				kind: 'outer'
			},
			{
				name: 'outliers',
				args: ["'dbscan'", '3'],
				argNames: ['algorithm', 'tolerance'],
				description:
					"Keeps only series unlike the rest: 'dbscan' or 'mad'; higher tolerance, fewer outliers."
			}
		]
	}
];

const SPECS = new Map(FUNCTION_FAMILIES.flatMap((f) => f.functions.map((s) => [s.name, s])));

export function functionSpec(name: string): FunctionSpec | undefined {
	return SPECS.get(name);
}

export function isModifier(call: FunctionCall): boolean {
	return functionSpec(call.name)?.kind === 'modifier';
}

/**
 * Adds a function to a row's list. anomalies() and forecast() must stay
 * outermost, so a new function goes in before one of them, and a second one
 * replaces the first.
 */
export function addFunction(functions: FunctionCall[], spec: FunctionSpec): FunctionCall[] {
	const call = { name: spec.name, args: [...spec.args] };
	const outerIndex = functions.findIndex((f) => functionSpec(f.name)?.kind === 'outer');

	if (spec.kind === 'outer') {
		return outerIndex === -1
			? [...functions, call]
			: functions.map((f, i) => (i === outerIndex ? call : f));
	}
	if (outerIndex === -1) return [...functions, call];
	return [...functions.slice(0, outerIndex), call, ...functions.slice(outerIndex)];
}

/** `.rollup(sum, 60).as_count()`: the modifiers among a row's functions. */
export function modifierText(functions: FunctionCall[]): string {
	return functions
		.filter(isModifier)
		.map((f) => `.${f.name}(${f.args.join(', ')})`)
		.join('');
}

/** `anomalies(abs(a), 'basic', 2)`: the other functions wrapped around an expression, first innermost. */
export function wrap(expression: string, functions: FunctionCall[]): string {
	return functions
		.filter((f) => !isModifier(f))
		.reduce((inner, f) => `${f.name}(${[inner, ...f.args].join(', ')})`, expression);
}
