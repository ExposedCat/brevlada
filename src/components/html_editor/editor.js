function cleanHTML(html) {
    const parsed = new DOMParser().parseFromString(html, 'text/html');
    parsed.querySelectorAll('script,style,link,meta,base,iframe,object,embed,form,input,button,textarea,select,svg,math,template').forEach(node => node.remove());
    for (const node of parsed.body.querySelectorAll('*')) {
        for (const attribute of [...node.attributes]) {
            if (!['style', 'href', 'src', 'alt', 'title', 'width', 'height', 'colspan', 'rowspan', 'align', 'valign', 'border', 'cellpadding', 'cellspacing', 'color', 'size', 'face'].includes(attribute.name)) {
                node.removeAttribute(attribute.name);
            }
        }
        if (node.hasAttribute('href') && !/^(https?:|mailto:)/i.test(node.getAttribute('href'))) node.removeAttribute('href');
        if (node.hasAttribute('src') && !/^data:image\/(png|jpeg|gif|webp);base64,/i.test(node.getAttribute('src'))) node.removeAttribute('src');
    }
    return parsed.body.innerHTML;
}
function quoteDocument(html) {
    const parsed = new DOMParser().parseFromString(html, 'text/html');
    parsed.querySelectorAll('script,link,meta,base,iframe,object,embed,form,input,button,textarea,select').forEach(node => node.remove());
    for (const node of parsed.querySelectorAll('*')) {
        for (const attribute of [...node.attributes]) {
            if (attribute.name.startsWith('on') || attribute.name === 'srcdoc') node.removeAttribute(attribute.name);
        }
    }
    const policy = parsed.createElement('meta');
    policy.httpEquiv = 'Content-Security-Policy';
    policy.content = "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:;";
    parsed.head.prepend(policy);
    const defaults = parsed.createElement('style');
    defaults.textContent = quoteStyle;
    policy.after(defaults);
    return '<!doctype html>' + parsed.documentElement.outerHTML;
}
function report() {
    const quote = document.querySelector('iframe');
    let text = document.body.innerText;
    if (quote?.contentDocument?.body) {
        text += quote.contentDocument.body.innerText.split('\n').map(line => '> ' + line).join('\n');
    }
    window.webkit.messageHandlers.composeText.postMessage(text);
}
function setContent(content) {
    document.body.innerHTML = cleanHTML(content.html) || '<div><br></div>';
    if (content.quote !== undefined) {
        const block = document.createElement('blockquote');
        block.contentEditable = 'false';
        const frame = document.createElement('iframe');
        frame.title = 'Quoted message';
        frame.setAttribute('sandbox', 'allow-same-origin');
        frame.srcdoc = quoteDocument(content.quote);
        frame.addEventListener('load', () => {
            const doc = frame.contentDocument;
            if (!doc?.body) return;
            const resize = () => {
                const height = Math.ceil(Math.max(doc.body.scrollHeight, doc.body.getBoundingClientRect().height));
                if (frame.height !== String(height)) frame.height = String(height);
            };
            new ResizeObserver(resize).observe(doc.body);
            resize();
            doc.addEventListener('click', event => event.preventDefault());
            report();
        });
        block.append(frame);
        document.body.append(block);
    }
    document.body.focus({preventScroll: true});
    const range = document.createRange();
    range.selectNodeContents(document.body.firstChild);
    range.collapse(true);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    window.scrollTo(0, 0);
    requestAnimationFrame(() => window.scrollTo(0, 0));
    report();
}
document.body.addEventListener('input', report);
document.body.addEventListener('click', event => {
    if (event.target.closest('a')) event.preventDefault();
});
document.body.addEventListener('paste', event => {
    event.preventDefault();
    const html = event.clipboardData.getData('text/html');
    if (html) document.execCommand('insertHTML', false, cleanHTML(html));
    else document.execCommand('insertText', false, event.clipboardData.getData('text/plain'));
    report();
});
document.body.addEventListener('drop', event => event.preventDefault());
document.body.addEventListener('keydown', event => {
    if (!event.ctrlKey && !event.metaKey) return;
    let command;
    if (event.shiftKey && event.key.toLowerCase() === 'x') command = 'strikeThrough';
    if (event.key === '\\') command = 'removeFormat';
    if (command) {
        event.preventDefault();
        document.execCommand(command);
        report();
    }
});
