/**
 * The appearance the configuration promises, as CSS variables.
 *
 * The `appearance.*` and `font.*` settings have been in the configuration
 * reference and the settings screen since either existed, and the client
 * rendered its built-in face regardless: no font key was ever read, the
 * cursor was always a block, and `appearance.theme_mode` travelled through
 * `theme.get`, which never consulted it either (that half is fixed on the
 * server).
 *
 * This module is deliberately pure: values in, CSS variables out, no DOM
 * knowledge beyond the root element. The rules that consume the variables
 * carry their old values as defaults, so a client with no configuration and
 * a client that never calls this render exactly what they always did.
 */
/**
 * Apply every appearance setting a resolved config carries.
 *
 * Each key is read by a direct bracket access, on purpose: the settings gate
 * counts a key as read when it appears inside a lookup, and an indirection
 * that hides a read would hide a broken key from the gate that exists to
 * find them.
 */
export function applyAppearance(root, values) {
    const family = values['appearance.font.family'] ?? values['font.family'];
    if (typeof family === 'string' && family.length > 0) {
        // The machine face is the terminal's own; the human face that surrounds
        // it is the app's and is not a setting.
        root.style.setProperty('--face-machine', family);
    }
    const size = number(values['appearance.font.size'] ?? values['font.size']);
    if (size !== undefined) {
        root.style.setProperty('--omt-font-size', `${size}px`);
    }
    const lineHeight = number(values['appearance.font.line_height'] ?? values['font.line_height']);
    if (lineHeight !== undefined) {
        root.style.setProperty('--omt-line-height', String(lineHeight));
    }
    // Ligatures are a rendering choice, and "off" is the safe spelling: a
    // terminal whose `fi` got welded together is not a terminal that was asked.
    // Only absent is absent: an unset key must leave the variable untouched,
    // so an unconfigured client renders exactly what it always did.
    const ligatures = values['appearance.font.ligatures'];
    if (ligatures === true || ligatures === false) {
        root.style.setProperty('--omt-ligatures', ligatures ? 'normal' : 'none');
    }
    const fallbacks = values['appearance.font.fallbacks'];
    if (Array.isArray(fallbacks) && fallbacks.length > 0) {
        const extra = fallbacks.filter((f) => typeof f === 'string');
        if (extra.length > 0) {
            const current = root.style.getPropertyValue('--face-machine').trim();
            root.style.setProperty('--face-machine', current ? `${current}, ${extra.join(', ')}` : extra.join(', '));
        }
    }
    // The cursor: shape and blink. The CSS rule for `.cursor` carries today's
    // block cursor as the default, so an unset cursor renders unchanged.
    const style = values['appearance.cursor.style'];
    if (style === 'bar') {
        root.style.setProperty('--omt-cursor-width', '2px');
        root.style.setProperty('--omt-cursor-height', '1.25em');
        root.style.setProperty('--omt-cursor-top', '0');
    }
    else if (style === 'underline') {
        root.style.setProperty('--omt-cursor-width', '1ch');
        root.style.setProperty('--omt-cursor-height', '2px');
        root.style.setProperty('--omt-cursor-top', 'calc(1.25em - 2px)');
    }
    if (values['appearance.cursor.blink'] === true || values['appearance.cursor.blink'] === false) {
        root.style.setProperty('--omt-cursor-blink', values['appearance.cursor.blink'] === true ? '1s steps(1) infinite' : 'none');
    }
    const density = values['appearance.density'];
    if (density === 'compact' || density === 'spacious') {
        root.style.setProperty('--omt-density', density);
    }
    // The block cards: gutter is the thick left rule they have always worn,
    // bordered is a uniform frame, minimal is a top rule and nothing else.
    const blockStyle = values['appearance.block_style'];
    if (blockStyle === 'gutter') {
        root.style.setProperty('--omt-block-left', '3px solid var(--border-strong)');
        root.style.setProperty('--omt-block-edge', '1px solid var(--border)');
        root.style.setProperty('--omt-block-top', 'none');
    }
    else if (blockStyle === 'bordered') {
        root.style.setProperty('--omt-block-left', '1px solid var(--border)');
        root.style.setProperty('--omt-block-edge', '1px solid var(--border)');
        root.style.setProperty('--omt-block-top', 'none');
    }
    else if (blockStyle === 'minimal') {
        root.style.setProperty('--omt-block-left', 'none');
        root.style.setProperty('--omt-block-edge', 'none');
        root.style.setProperty('--omt-block-top', '1px solid var(--border)');
    }
}
/** A value as a finite number, or nothing. */
function number(value) {
    if (typeof value === 'number' && Number.isFinite(value)) {
        return value;
    }
    return undefined;
}
