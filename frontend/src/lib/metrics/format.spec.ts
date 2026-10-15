import { describe, expect, it } from 'vitest';
import { formatValue } from './format';

describe('formatValue', () => {
	it.each([
		[0, '0'],
		[12.345, '12.35'],
		[1337, '1.34k'],
		[-2500, '-2.5k'],
		[11_771_153_846, '11.77G'],
		[0.0042, '0.0042'],
		[0.001, '0.001'],
		[999_999, '1M'],
		[0.000042, '4.2e-5']
	])('%s is %s', (v, text) => {
		expect(formatValue(v)).toBe(text);
	});
});
