(() => {
    const root = document.getElementById('brevlada-content');
    if (!root || window.brevladaQuotes) return;
    window.brevladaQuotes = true;
    const quotes = [];
    const trailing = node => {
        if (node.nodeType === Node.TEXT_NODE) return !node.textContent.trim();
        if (node.nodeType !== Node.ELEMENT_NODE) return true;
        if (node.matches('script, style, br')) return true;
        if (node.matches('blockquote, .gmail_quote, .yahoo_quoted, .protonmail_quote')) {
            quotes.push(node);
            return true;
        }
        if (node.matches('img, svg, video, audio, iframe, object, hr, input, details')) return false;
        for (const child of Array.from(node.childNodes).reverse()) {
            if (!trailing(child)) return false;
        }
        return true;
    };
    trailing(root);
    for (const quote of quotes) {
        if (root.dataset.hideQuotes === 'true') {
            quote.remove();
            continue;
        }
        const details = document.createElement('details');
        const summary = document.createElement('summary');
        summary.textContent = 'Quoted reply';
        quote.before(details);
        details.append(summary, quote);
    }
})();
