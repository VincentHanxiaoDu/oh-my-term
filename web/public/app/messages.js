/**
 * What omt says, in the reader's language.
 *
 * One rule governs the whole file, and it follows from the two typefaces the
 * client already uses: **omt's words are localizable and the agent's words are
 * never touched.** A card's options are the agent's; the button that submits
 * them is omt's. A client that translated the options would be putting words
 * in the agent's mouth on the one screen where exactness is the product.
 *
 * English is the *type*. Every other language is `Partial<Messages>`, which
 * makes the fallback total by construction — there is no way to add a key
 * without English having it, and no way for a lookup to find nothing.
 *
 * No phrase is assembled by concatenation. `${n} waiting on you` is a sentence
 * in English and is not one in Chinese: word order differs, plural rules
 * differ, and the concatenating version is wrong everywhere except where it
 * was written. So a phrase with a number in it is a *function* per language.
 */
const table = {
    'projects.title': 'Open a project',
    'projects.emptyTitle': 'No projects yet',
    'projects.emptyDetail': 'Run omt in a directory to open it as a project.',
    'projects.openIn': (name) => `Start a session in ${name}`,
    'worktree.remove': 'Remove this worktree',
    'worktree.removeAnyway': 'Remove it anyway, discarding that work',
    'worktree.keep': 'Keep it',
    'projects.newDirectory': 'To open a directory omt has not seen, run omt in it. This list only offers what the instance already knows, because a path typed on a phone cannot be checked before it is wrong.',
    'projects.gone': 'this directory is gone',
    'attach.label': 'Attach',
    'attach.photo': 'Take a photo',
    'attach.file': 'Choose a file',
    'attach.paste': 'Paste an image',
    'attach.workspaceFile': 'A file in this project',
    'attach.pick': 'Choose a file to attach',
    'attach.cancel': 'Cancel',
    'attach.noImage': 'The clipboard has no image in it.',
    'attach.sending': (name) => `sending ${name}…`,
    'attach.failed': (name, reason) => `${name} did not send: ${reason}`,
    'files.title': 'Files',
    'files.empty': 'This directory is empty.',
    'files.tooLarge': (bytes) => `Too large to show here (${bytes} bytes).`,
    'files.binary': 'Binary file.',
    'changes.title': 'Changes',
    'changes.clean': 'Nothing has changed.',
    'changes.noRepository': 'This project is not a git repository, so there is nothing to compare.',
    'changes.binary': 'Binary file — no diff to show.',
    'changes.tooLarge': 'Too large to diff here.',
    'changes.noDiff': 'No diff for this file.',
    'settings.title': 'Settings',
    'settings.default': 'default',
    'settings.from': (layer) => `set in ${layer}`,
    'usage.tokens': (input, output) => `${input} in · ${output} out`,
    'usage.context': (percent) => `${percent}% of context`,
    'usage.noCost': 'no cost reported',
    'orphan.explain': 'This session ended when omt did. Its output is here to read; nothing is listening for what you type.',
    'orphan.restart': 'Run it again',
    'orphan.close': 'Close this session',
    'orphan.clear': (n) => n === 1 ? 'Close 1 ended session' : `Close ${n} ended sessions`,
    'orphan.cleared': (n) => n === 1 ? '1 session closed' : `${n} sessions closed`,
    'language.name': 'Language',
    'language.system': 'Follow the browser',
};
/** Every string omt owns. */
export const en = table;
/**
 * Simplified Chinese.
 *
 * `Partial`, deliberately: a key with no entry falls back to English, which is
 * usable. Rendering the key instead is how a half-translated screen ships, and
 * a screen of identifiers is a screen nobody can use.
 */
export const zhHans = {
    'projects.title': '打开项目',
    'projects.emptyTitle': '还没有项目',
    'projects.emptyDetail': '在某个目录里运行 omt，它就会成为一个项目。',
    'projects.openIn': (name) => `在 ${name} 中新建会话`,
    'worktree.remove': '删除此工作树',
    'worktree.removeAnyway': '仍然删除，丢弃这些改动',
    'worktree.keep': '保留',
    'projects.newDirectory': '要打开 omt 没见过的目录，请在该目录中运行 omt。这里只列出实例已知的位置——在手机上输入的路径，无法在出错之前被验证。',
    'projects.gone': '该目录已不存在',
    'attach.label': '附件',
    'attach.photo': '拍照',
    'attach.file': '选择文件',
    'attach.paste': '粘贴图片',
    'attach.workspaceFile': '项目中的文件',
    'attach.pick': '选择要附加的文件',
    'attach.cancel': '取消',
    'attach.noImage': '剪贴板里没有图片。',
    'attach.sending': (name) => `正在发送 ${name}…`,
    'attach.failed': (name, reason) => `${name} 发送失败：${reason}`,
    'files.title': '文件',
    'files.empty': '这个目录是空的。',
    'files.tooLarge': (bytes) => `文件过大，无法在此显示（${bytes} 字节）。`,
    'files.binary': '二进制文件。',
    'changes.title': '改动',
    'changes.clean': '没有任何改动。',
    'changes.noRepository': '这个项目不是 git 仓库，没有可比较的内容。',
    'changes.binary': '二进制文件——没有可显示的差异。',
    'changes.tooLarge': '文件过大，无法在此显示差异。',
    'changes.noDiff': '这个文件没有差异。',
    'settings.title': '设置',
    'settings.default': '默认值',
    'settings.from': (layer) => `来自 ${layer}`,
    'usage.tokens': (input, output) => `输入 ${input} · 输出 ${output}`,
    'usage.context': (percent) => `已用上下文 ${percent}%`,
    'usage.noCost': '智能体未报告费用',
    'orphan.explain': '这个会话随 omt 一起结束了。输出还在，可以查看；但没有进程在接收你的输入。',
    'orphan.restart': '重新运行',
    'orphan.close': '关闭此会话',
    'orphan.clear': (n) => `关闭 ${n} 个已结束的会话`,
    'orphan.cleared': (n) => `已关闭 ${n} 个会话`,
    'language.name': '语言',
    'language.system': '跟随浏览器',
};
/** The languages this client speaks. */
export const languages = { en, 'zh-Hans': zhHans };
/**
 * Which language to render in.
 *
 * From the browser, because that is what somebody already told their device,
 * and overridable because a device's language and a person's preference are
 * not always the same thing. Never from the instance: language is a property
 * of the reader, not of the machine, and two people looking at one instance
 * may legitimately want different ones.
 */
export function chooseLanguage(stored, preferred) {
    if (stored !== null && stored in languages) {
        return stored;
    }
    for (const tag of preferred) {
        // `zh`, `zh-CN`, `zh-Hans-CN` all mean the same thing to this client, and
        // matching only the exact tag would leave most Chinese browsers in English.
        if (/^zh\b/i.test(tag)) {
            return 'zh-Hans';
        }
    }
    return 'en';
}
let active = 'en';
/** Set the language for everything rendered after this. */
export function setLanguage(tag) {
    active = tag;
}
/** Which language is in use. */
export function language() {
    return active;
}
export function t(key, ...args) {
    const table = languages[active];
    const value = (table[key] ?? en[key]);
    if (typeof value === 'function') {
        return value(...args);
    }
    return value;
}
/**
 * How many columns a string occupies in a terminal-shaped layout.
 *
 * Not its length. A CJK glyph is one code point and two columns wide, so a
 * layout that truncated by `String.length` cut Chinese text to half the width
 * it reserved — and this client measures the status line, the roster's
 * truncation and the terminal grid in columns already.
 *
 * The ranges are the East Asian Wide and Fullwidth blocks, which is what every
 * terminal uses and is enough: the cases it does not cover — combining marks,
 * emoji sequences — are not what the layouts here are cut against.
 */
export function columns(text) {
    let total = 0;
    for (const ch of text) {
        const code = ch.codePointAt(0) ?? 0;
        total += isWide(code) ? 2 : 1;
    }
    return total;
}
function isWide(code) {
    return ((code >= 0x1100 && code <= 0x115f) || // Hangul Jamo
        (code >= 0x2e80 && code <= 0xa4cf) || // CJK radicals through Yi
        (code >= 0xac00 && code <= 0xd7a3) || // Hangul syllables
        (code >= 0xf900 && code <= 0xfaff) || // CJK compatibility ideographs
        (code >= 0xfe30 && code <= 0xfe6f) || // CJK compatibility forms
        (code >= 0xff00 && code <= 0xff60) || // Fullwidth forms
        (code >= 0xffe0 && code <= 0xffe6) ||
        (code >= 0x1f300 && code <= 0x1f64f) || // Emoji
        (code >= 0x20000 && code <= 0x3fffd) // CJK extensions B–F
    );
}
/**
 * Cut a string to a column width, not a character count.
 *
 * The ellipsis costs a column and is included, so the result never exceeds the
 * width it was given — which is what the caller reserved and what the next
 * thing on the row is drawn at.
 */
export function truncate(text, width) {
    if (columns(text) <= width) {
        return text;
    }
    if (width <= 1) {
        return width === 1 ? '…' : '';
    }
    let out = '';
    let used = 0;
    for (const ch of text) {
        const w = isWide(ch.codePointAt(0) ?? 0) ? 2 : 1;
        if (used + w > width - 1) {
            break;
        }
        out += ch;
        used += w;
    }
    return `${out}…`;
}
