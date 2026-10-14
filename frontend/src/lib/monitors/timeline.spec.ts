import { describe, expect, it } from 'vitest';
import { timelines } from './timeline';

const at = (minute: number) => new Date(Date.UTC(2026, 9, 14, 12, minute)).toISOString();
const t = (minute: number) => Date.parse(at(minute));

describe('timelines', () => {
	it('starts a group in the state it entered the range in', () => {
		const rows = timelines(
			[{ group: 'host:a', at: at(0), fromState: 'OK', toState: 'Alert', reason: 'threshold' }],
			[{ group: 'host:a', at: at(30), fromState: 'Alert', toState: 'OK', reason: 'recovery' }],
			t(10),
			t(60)
		);
		expect(rows).toEqual([
			{
				group: 'host:a',
				segments: [
					{ state: 'Alert', from: t(10), to: t(30) },
					{ state: 'OK', from: t(30), to: t(60) }
				]
			}
		]);
	});

	it('leaves a group blank before its first transition and after it closes', () => {
		const rows = timelines(
			[],
			[
				{ group: 'host:b', at: at(20), fromState: null, toState: 'Warn', reason: 'new' },
				{ group: 'host:b', at: at(40), fromState: 'Warn', toState: 'OK', reason: 'expired' }
			],
			t(0),
			t(60)
		);
		expect(rows[0].segments).toEqual([
			{ state: null, from: t(0), to: t(20) },
			{ state: 'Warn', from: t(20), to: t(40) },
			{ state: null, from: t(40), to: t(60) }
		]);
	});

	it('puts the worst state first', () => {
		const rows = timelines(
			[
				{ group: 'host:a', at: at(0), fromState: null, toState: 'OK', reason: 'new' },
				{ group: 'host:c', at: at(0), fromState: 'OK', toState: 'Alert', reason: 'threshold' },
				{ group: 'host:b', at: at(0), fromState: 'OK', toState: 'No Data', reason: 'no_data' }
			],
			[],
			t(10),
			t(20)
		);
		expect(rows.map((r) => r.group)).toEqual(['host:c', 'host:b', 'host:a']);
	});

	it('claims nothing before the oldest transition kept when the rest were cut off', () => {
		const rows = timelines(
			[{ group: 'host:a', at: at(0), fromState: null, toState: 'OK', reason: 'new' }],
			[
				{ group: 'host:a', at: at(40), fromState: 'Warn', toState: 'Alert', reason: 'threshold' },
				{ group: 'host:b', at: at(30), fromState: 'OK', toState: 'Warn', reason: 'threshold' }
			],
			t(10),
			t(60),
			true
		);
		expect(rows).toEqual([
			{
				group: 'host:a',
				segments: [
					{ state: null, from: t(10), to: t(30) },
					{ state: 'Warn', from: t(30), to: t(40) },
					{ state: 'Alert', from: t(40), to: t(60) }
				]
			},
			{
				group: 'host:b',
				segments: [
					{ state: null, from: t(10), to: t(30) },
					{ state: 'Warn', from: t(30), to: t(60) }
				]
			}
		]);
	});
});
