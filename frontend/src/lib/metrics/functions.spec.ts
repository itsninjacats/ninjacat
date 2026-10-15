import { describe, expect, it } from 'vitest';
import {
	FUNCTION_FAMILIES,
	addFunction,
	functionSpec,
	modifierText,
	wrap,
	type FunctionCall
} from './functions';

const spec = (name: string) => functionSpec(name)!;

describe('wrap', () => {
	it('applies functions first innermost, skipping modifiers', () => {
		const fns: FunctionCall[] = [
			{ name: 'abs', args: [] },
			{ name: 'rollup', args: ['sum', '60'] },
			{ name: 'top', args: ['10', "'mean'", "'desc'"] }
		];
		expect(wrap('a', fns)).toBe("top(abs(a), 10, 'mean', 'desc')");
		expect(modifierText(fns)).toBe('.rollup(sum, 60)');
	});
});

describe('addFunction', () => {
	it('keeps anomalies and forecast outermost', () => {
		let fns = addFunction([], spec('anomalies'));
		fns = addFunction(fns, spec('abs'));
		expect(fns.map((f) => f.name)).toEqual(['abs', 'anomalies']);
	});

	it('lets only one outer function stay', () => {
		let fns = addFunction([], spec('anomalies'));
		fns = addFunction(fns, spec('forecast'));
		expect(fns.map((f) => f.name)).toEqual(['forecast']);
	});

	it('copies the default arguments, so editing one call leaves the catalog alone', () => {
		const [call] = addFunction([], spec('round'));
		call.args[0] = '5';
		expect(spec('round').args).toEqual(['2']);
	});
});

describe('the catalog', () => {
	it('describes every function and names every argument', () => {
		for (const spec of FUNCTION_FAMILIES.flatMap((f) => f.functions)) {
			expect(spec.description, spec.name).not.toBe('');
			expect(spec.argNames ?? [], spec.name).toHaveLength(spec.args.length);
		}
	});
});
