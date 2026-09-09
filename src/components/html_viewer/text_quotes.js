(() => {
    const root = document.getElementById('brevlada-content');
    if (!root || window.brevladaTextQuotes) return;
    window.brevladaTextQuotes = true;
    const lines = [];
    let line = {text: '', start: null, end: null};
    const newline = () => {
        lines.push(line);
        line = {text: '', start: null, end: null};
    };
    const text = (node, value, offset) => {
        if (!line.start && value.trim()) {
            line.start = [node, offset + value.search(/\S/)];
        }
        line.text += value;
        if (value.trim()) line.end = [node, offset + value.trimEnd().length];
    };
    const visit = node => {
        if (node.nodeType === Node.TEXT_NODE) {
            const whitespace = getComputedStyle(node.parentElement).whiteSpace;
            if (['pre', 'pre-wrap', 'pre-line', 'break-spaces'].includes(whitespace)) {
                let offset = 0;
                const parts = node.textContent.split('\n');
                parts.forEach((part, index) => {
                    if (index) newline();
                    text(node, part, offset);
                    offset += part.length + 1;
                });
            } else {
                text(node, node.textContent, 0);
            }
            return;
        }
        if (node.nodeType !== Node.ELEMENT_NODE) return;
        if (node.matches('script, style')) return;
        if (node.matches('br')) return newline();
        if (node.matches('blockquote, details, img, svg, video, audio, iframe, object, hr, input')) {
            newline();
            line.text = '\uFFFC';
            newline();
            return;
        }
        const block = !['inline', 'contents'].includes(getComputedStyle(node).display);
        if (block) newline();
        for (const child of node.childNodes) visit(child);
        if (block) newline();
    };
    visit(root);
    newline();
    let first = null;
    let last = null;
    for (const candidate of lines.reverse()) {
        const value = candidate.text.trim();
        if (!value) continue;
        if (!value.startsWith('>')) break;
        first = candidate.start;
        last ??= candidate.end;
    }
    if (!first || !last) return;
    const range = document.createRange();
    range.setStart(...first);
    range.setEnd(...last);
    const details = document.createElement('details');
    const summary = document.createElement('summary');
    summary.textContent = 'Quoted reply';
    details.append(summary, range.extractContents());
    range.insertNode(details);
})();
