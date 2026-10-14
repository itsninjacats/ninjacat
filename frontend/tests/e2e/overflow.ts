import { expect, type Page, type TestInfo } from '@playwright/test';

export type Overflow = {
	// page-scroll: the page (or the app's content pane) scrolls sideways.
	// spills:      content sticks out of a box that does not clip or scroll it.
	// clipped:     text or a control is cut off, with no ellipsis and no way to read the rest.
	// off-screen:  an element reaches past the viewport's left or right edge.
	// truncated:   text ends in an ellipsis, and no title (no tap, on touch) shows the rest.
	// hidden:      a box scrolls sideways and hides text or controls past its edge.
	// squeezed:    text wraps into a column a few characters wide.
	// cramped:     text touches the border around it.
	// misaligned:  an item of a centred row sits off the row's centre line.
	rule:
		| 'page-scroll'
		| 'spills'
		| 'clipped'
		| 'off-screen'
		| 'truncated'
		| 'hidden'
		| 'squeezed'
		| 'cramped'
		| 'misaligned';
	element: string;
	text: string;
	detail: string;
};

// The app shell fixes the page to the viewport's height and scrolls the content
// pane instead (src/routes/app/+layout.svelte), so that pane sideways is the
// page sideways: it is held to the page's rule, not let off as a scroll area.
export const contentPane = '[data-slot="sidebar-inset"] > .overflow-auto';

// Runs in the page, so it is self-contained: no imports, no closures.
function scan({ pane, touch }: { pane: string; touch: boolean }): Overflow[] {
	// What may stick out of its parent, and why: the sidebar's rail is a hit
	// area that straddles the sidebar's edge by design.
	const allowedToStickOut = '[data-sidebar="rail"]';
	// LayerChart pads its plot area off the axes and draws the chart's SVG from
	// inside it, so the SVG overhangs these boxes by design. They alone may be
	// exceeded: the SVG is still measured against the chart and the card around it.
	const chartPlotArea = '.lc-tooltip-context, .lc-tooltip-context-container';
	const tolerance = 1;
	const viewport = document.documentElement.clientWidth;
	const problems: Overflow[] = [];
	const controls = 'button, a, input, select, textarea, [role=button], [role=menuitem]';

	// A title is read by hovering, which a touch screen cannot do. There, cut text
	// passes only inside a control that opens a popup, where a tap shows it whole.
	const readable = (el: Element) =>
		touch ? el.closest('[aria-haspopup]') !== null : el.closest('[title]') !== null;

	function describe(el: Element): string {
		const parts: string[] = [];
		for (let at: Element | null = el; at && at !== document.body && parts.length < 5;) {
			let part = at.tagName.toLowerCase();
			const slot = at.getAttribute('data-slot') ?? at.getAttribute('data-sidebar');
			if (slot) part += `[data-slot=${slot}]`;
			const classes = [...at.classList].filter((c) => !c.includes(':')).slice(0, 3);
			if (classes.length > 0) part += '.' + classes.join('.');
			parts.unshift(part);
			at = at.parentElement;
		}
		return parts.join(' > ');
	}

	function textOf(node: Node): string {
		const raw = node instanceof HTMLElement ? node.innerText : (node.textContent ?? '');
		const text = raw.replace(/\s+/g, ' ').trim();
		return text.length > 120 ? text.slice(0, 117) + '...' : text;
	}

	const clips = (value: string) => value !== 'visible';
	const scrolls = (value: string) => value === 'auto' || value === 'scroll';
	const px = (value: string) => parseFloat(value) || 0;

	// Tailwind's sr-only: a box clipped to nothing, there for screen readers.
	function visuallyHidden(el: Element): boolean {
		for (let at: Element | null = el; at; at = at.parentElement) {
			const style = getComputedStyle(at);
			if (style.clipPath === 'inset(50%)' || style.clip === 'rect(0px, 0px, 0px, 0px)') return true;
		}
		return false;
	}

	function hidden(el: Element): boolean {
		if (!el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })) return true;
		const rect = el.getBoundingClientRect();
		return rect.width === 0 || rect.height === 0 || visuallyHidden(el);
	}

	// The nearest ancestor that clips or scrolls, below `limit`.
	function clipperOf(node: Node, limit: Element | null): Element | null {
		for (let at = node.parentElement; at && at !== limit; at = at.parentElement) {
			const style = getComputedStyle(at);
			if (clips(style.overflowX) || clips(style.overflowY)) return at;
			if (style.position === 'fixed') return null;
		}
		return null;
	}

	function rectsOf(node: Node): DOMRect[] {
		if (node instanceof Element) return [node.getBoundingClientRect()];
		const range = document.createRange();
		range.selectNodeContents(node);
		return [...range.getClientRects()];
	}

	function isBlank(node: Node): boolean {
		return node instanceof Text && node.data.trim() === '';
	}

	function skipped(node: Node): boolean {
		// An SVG is measured whole: what it draws inside is its own affair.
		if (node.parentElement?.closest('svg')) return true;
		const el = node instanceof Element ? node : node.parentElement!;
		if (el.closest(allowedToStickOut)) return true;
		return hidden(el);
	}

	// What reaches past `right` (and `bottom`, when given) inside `box`, and is
	// not held back by a clipping or scrolling box in between. `wanted` picks
	// what is reported; the rest is only looked into.
	function stickingOut(
		box: Element,
		right: number,
		bottom: number | null,
		wanted: (node: Node) => boolean = () => true
	): Node[] {
		const found: Node[] = [];
		const walker = document.createTreeWalker(box, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
		for (let node = walker.nextNode(); node; node = walker.nextNode()) {
			if (isBlank(node) || skipped(node)) continue;
			if (clipperOf(node, box) !== null) continue;
			if (found.some((outer) => outer.contains(node))) continue;
			if (!wanted(node)) continue;
			// A line of text is taller than a tight line-height (leading-none,
			// leading-[1.05]) by the font's own ascent and descent, which overhang
			// the box without being cut or overlapping anything. A quarter of the
			// line is well past that overhang and well short of a hidden line.
			const below = (r: DOMRect) =>
				bottom !== null &&
				(node instanceof Text ? r.bottom - r.height / 4 > bottom : r.bottom > bottom + tolerance);
			const out = rectsOf(node).some(
				(r) => r.width > 0 && (r.right > right + tolerance || below(r))
			);
			if (out) found.push(node);
		}
		return found;
	}

	// The ancestor of `el` that cuts `rect` off, if one does: a box that hides
	// its overflow, below the nearest one that scrolls.
	function cutBy(el: Element, rect: DOMRect): Element | null {
		for (let at = el.parentElement; at && at !== document.body; at = at.parentElement) {
			const style = getComputedStyle(at);
			if (scrolls(style.overflowX) || style.position === 'fixed') return null;
			if (clips(style.overflowX)) {
				const box = at.getBoundingClientRect();
				return rect.right > box.right + tolerance || rect.bottom > box.bottom + tolerance
					? at
					: null;
			}
		}
		return null;
	}

	// The width of `text` in the font `style` sets, for what is not in the DOM
	// as text: an input's value or placeholder, a select's chosen option.
	const context = document.createElement('canvas').getContext('2d')!;
	function textWidth(text: string, style: CSSStyleDeclaration): number {
		context.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
		context.letterSpacing = style.letterSpacing === 'normal' ? '0px' : style.letterSpacing;
		return context.measureText(text).width;
	}

	const textInputs = ['text', 'search', 'email', 'url', 'tel'];
	// Chromium draws a native select's arrow inside the content box, about this wide.
	const nativeArrow = 20;

	// A control that cannot show what it holds. While typing, an input scrolls
	// its value, as it should; at rest it has to say that it holds more.
	function cutControl(el: Element, style: CSSStyleDeclaration): string | null {
		let text: string;
		let room = el.clientWidth - px(style.paddingLeft) - px(style.paddingRight);
		if (el instanceof HTMLInputElement) {
			if (!textInputs.includes(el.type) || el === document.activeElement) return null;
			text = el.value || el.placeholder;
			if (style.textOverflow === 'ellipsis') return null;
		} else if (el instanceof HTMLSelectElement) {
			text = el.selectedOptions[0]?.text ?? '';
			if (style.appearance !== 'none') room -= nativeArrow;
		} else {
			return null;
		}
		const needed = textWidth(text, style);
		if (needed <= room + tolerance || readable(el)) return null;
		return `shows ${Math.round(room)}px of ${Math.round(needed)}px of text`;
	}

	const page = document.scrollingElement ?? document.documentElement;
	if (page.scrollWidth > page.clientWidth + tolerance) {
		problems.push({
			rule: 'page-scroll',
			element: 'html',
			text: '',
			detail: `the page is ${page.scrollWidth}px wide in a ${page.clientWidth}px viewport`
		});
	}

	const reported = new Map<Node, Overflow>();
	const offScreen: Element[] = [];

	for (const el of document.body.querySelectorAll('*')) {
		if (el.closest('svg') && el.tagName.toLowerCase() !== 'svg') continue;
		if (hidden(el)) continue;
		const rect = el.getBoundingClientRect();
		const style = getComputedStyle(el);

		if (rect.right > viewport + tolerance || rect.left < -tolerance) {
			const clipper = clipperOf(el, null);
			const contained =
				clipper !== null &&
				clipper.getBoundingClientRect().right <= viewport + tolerance &&
				clipper.getBoundingClientRect().left >= -tolerance;
			if (!contained && !offScreen.some((outer) => outer.contains(el))) offScreen.push(el);
		}

		if (el instanceof HTMLInputElement || el instanceof HTMLSelectElement) {
			const cut = cutControl(el, style);
			if (cut) {
				problems.push({
					rule: 'clipped',
					element: describe(el),
					text:
						el instanceof HTMLSelectElement
							? (el.selectedOptions[0]?.text ?? '')
							: el.value || el.placeholder,
					detail: cut
				});
			}
		}
		// A replaced element has no content box to overflow.
		if (el.matches('input, select, textarea, img, video, canvas, iframe, svg')) continue;
		const wide = el.scrollWidth > el.clientWidth + tolerance;
		const tall = el.scrollHeight > el.clientHeight + tolerance;
		if (!wide && !tall) continue;
		if (style.display === 'inline' || style.display === 'contents') continue;
		if (el.matches(chartPlotArea)) continue;

		const isPane = el.matches(pane);
		if (isPane && wide) {
			problems.push({
				rule: 'page-scroll',
				element: describe(el),
				text: '',
				detail: `the content pane is ${el.scrollWidth}px wide in ${el.clientWidth}px`
			});
		}
		if ((wide && scrolls(style.overflowX)) || (tall && scrolls(style.overflowY))) {
			if (isPane) continue;
			// A table or a key that scrolls sideways shows no sign that it does
			// (scrollbars overlay), so what it hides there is as good as gone. A
			// listing of code scrolls by convention.
			if (wide && scrolls(style.overflowX) && el.closest('pre') === null) {
				const visible = rect.left + el.clientLeft + el.clientWidth;
				const beyond = stickingOut(
					el,
					visible,
					null,
					(node) => node instanceof Text || (node as Element).matches(`${controls}, th`)
				);
				if (beyond.length > 0) {
					problems.push({
						rule: 'hidden',
						element: describe(el),
						text: beyond
							.slice(0, 4)
							.map(
								(node) =>
									textOf(node) ||
									describe(node as Element)
										.split(' > ')
										.pop()
							)
							.join(' | '),
						detail:
							`scrolls sideways (${el.scrollWidth}px in ${el.clientWidth}px) and hides ` +
							`${beyond.length} ${beyond.length === 1 ? 'thing' : 'things'} past its right edge`
					});
				}
			}
			continue;
		}

		const clipping = clips(style.overflowX) || clips(style.overflowY);
		if (clipping && style.textOverflow === 'ellipsis') {
			// Truncation that says so; the rest has to be a hover or a tap away.
			if (wide && !readable(el)) {
				problems.push({
					rule: 'truncated',
					element: describe(el),
					text: textOf(el),
					detail: `shows ${el.clientWidth}px of ${el.scrollWidth}px, and ${touch ? 'no tap' : 'no title'} shows the rest`
				});
			}
			continue;
		}
		if (clipping && readable(el)) continue;

		const right = rect.right - px(style.borderRightWidth);
		const bottom = rect.bottom - px(style.borderBottomWidth);
		for (const node of stickingOut(el, right, bottom)) {
			const control = node instanceof Element && node.matches(controls);
			if (clipping && !(node instanceof Text) && !control) continue;
			const shown = node instanceof Text ? node.parentElement! : (node as Element);
			const r = rectsOf(node).reduce((a, b) => (a.right > b.right ? a : b));
			// A box that lets its content out may itself sit in one that cuts it.
			const cutter = clipping ? el : cutBy(el, r);
			const edge = (cutter ?? el).getBoundingClientRect();
			// Visited outermost first: a deeper box replaces its ancestor's report.
			reported.set(node, {
				rule: cutter ? 'clipped' : 'spills',
				element: describe(shown),
				text: textOf(node),
				detail:
					`${cutter ? 'cut off by' : 'sticks out of'} ${describe(cutter ?? el)}: ` +
					`${Math.round(Math.max(r.right - edge.right, 0))}px past its right edge, ` +
					`${Math.round(Math.max(r.bottom - edge.bottom, 0))}px past its bottom`
			});
		}
	}

	problems.push(...reported.values());
	for (const el of offScreen) {
		const r = el.getBoundingClientRect();
		problems.push({
			rule: 'off-screen',
			element: describe(el),
			text: textOf(el),
			detail: `spans ${Math.round(r.left)}px to ${Math.round(r.right)}px of a ${viewport}px viewport`
		});
	}

	// The rules below look at the text itself: how it wraps, and what is next to it.
	const squeezed = new Set<Element>();
	const cramped = new Set<Element>();
	const texts = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
	for (let node = texts.nextNode(); node; node = texts.nextNode()) {
		const parent = node.parentElement!;
		if (isBlank(node) || parent.closest('svg, script, style') || skipped(node)) continue;
		const rects = rectsOf(node).filter((r) => r.width > 0);
		if (rects.length === 0) continue;

		let block = parent;
		while (block.parentElement && getComputedStyle(block).display.startsWith('inline')) {
			block = block.parentElement;
		}
		// Three lines in less than eight characters' width: a word or two a line.
		const lines = new Set(rects.map((r) => Math.round(r.top))).size;
		const width = block.getBoundingClientRect().width;
		if (lines >= 3 && width < 8 * px(getComputedStyle(block).fontSize) && !squeezed.has(block)) {
			squeezed.add(block);
			problems.push({
				rule: 'squeezed',
				element: describe(block),
				text: textOf(block),
				detail: `${lines} lines in ${Math.round(width)}px`
			});
		}

		// The nearest box with a side border, unless a clipping box comes first:
		// what that one cuts off, the rules above report.
		let framed: Element | null = null;
		for (let at: Element | null = parent; at && at !== document.body; at = at.parentElement) {
			const style = getComputedStyle(at);
			const sides =
				(style.borderLeftStyle !== 'none' && px(style.borderLeftWidth) > 0) ||
				(style.borderRightStyle !== 'none' && px(style.borderRightWidth) > 0);
			if (sides) {
				framed = at;
				break;
			}
			if (clips(style.overflowX) || style.position === 'fixed') break;
		}
		if (framed && !cramped.has(framed)) {
			const style = getComputedStyle(framed);
			const frame = framed.getBoundingClientRect();
			const left = frame.left + px(style.borderLeftWidth);
			const right = frame.right - px(style.borderRightWidth);
			const gap = Math.min(...rects.map((r) => Math.min(r.left - left, right - r.right)));
			// Below zero the text is out of the box, which the rules above report.
			if (gap > -tolerance && gap < 4) {
				cramped.add(framed);
				problems.push({
					rule: 'cramped',
					element: describe(framed),
					text: textOf(node),
					detail: `the text comes within ${Math.max(Math.round(gap), 0)}px of the border`
				});
			}
		}
	}

	for (const row of document.body.querySelectorAll('*')) {
		const style = getComputedStyle(row);
		if (!style.display.endsWith('flex') || !style.flexDirection.startsWith('row')) continue;
		if (style.flexWrap !== 'nowrap' || style.alignItems !== 'center' || hidden(row)) continue;
		const box = row.getBoundingClientRect();
		const top = box.top + px(style.borderTopWidth) + px(style.paddingTop);
		const bottom = box.bottom - px(style.borderBottomWidth) - px(style.paddingBottom);
		const centre = (top + bottom) / 2;
		for (const item of row.children) {
			const own = getComputedStyle(item);
			if (own.position === 'absolute' || own.position === 'fixed') continue;
			if (own.display === 'contents' || own.transform !== 'none' || hidden(item)) continue;
			const r = item.getBoundingClientRect();
			const middle = (r.top - px(own.marginTop) + r.bottom + px(own.marginBottom)) / 2;
			if (Math.abs(middle - centre) > tolerance) {
				problems.push({
					rule: 'misaligned',
					element: describe(item),
					text: textOf(item),
					detail: `its centre is ${Math.round(middle - centre)}px off the centre of ${describe(row)}`
				});
			}
		}
	}

	return problems;
}

// Waits for transitions (sheets, dialogs, menus) and web fonts to finish, so
// what is measured is what stays on screen.
export async function settle(page: Page): Promise<void> {
	await page.evaluate(() => document.fonts.ready);
	await page.waitForFunction(() =>
		document
			.getAnimations()
			.every((a) => a.playState !== 'running' || a.effect?.getTiming().iterations === Infinity)
	);
}

export async function expectNoOverflow(
	page: Page,
	testInfo: TestInfo,
	label: string
): Promise<void> {
	await settle(page);
	const problems = await page.evaluate(scan, {
		pane: contentPane,
		touch: testInfo.project.use.hasTouch ?? false
	});
	const report = problems.map((p) => `[${p.rule}] ${p.element}\n    "${p.text}"\n    ${p.detail}`);
	expect.soft(report, `${label}: nothing overflows`).toEqual([]);
}
