function restoreMessageSpacing(root, state) {
    for (const owner of root.querySelectorAll('[data-brevlada-inset]')) {
        const original = state.entries[Number(owner.dataset.brevladaInset)].originalStyle;
        if (original === null) owner.removeAttribute('style');
        else owner.setAttribute('style', original);
        owner.removeAttribute('data-brevlada-inset');
    }
    for (const wrapper of root.querySelectorAll('[data-brevlada-spacing-wrapper]')) {
        wrapper.replaceWith(...wrapper.childNodes);
    }
}
function addMessageSpacing(doc) {
    const body = doc.body;
    if (body.querySelector('[data-plain-text="true"]')) return;
    const viewport = doc.documentElement.clientWidth;
    if (viewport <= 0 || body.getBoundingClientRect().height <= 0) return;
    const style = element => doc.defaultView.getComputedStyle(element);
    const gaps = owner => {
        const bounds = owner.getBoundingClientRect();
        const css = style(owner);
        return {
            left: bounds.left + parseFloat(css.borderLeftWidth) + parseFloat(css.paddingLeft),
            right: viewport - bounds.right + parseFloat(css.borderRightWidth) + parseFloat(css.paddingRight),
            top: bounds.top + doc.defaultView.scrollY + parseFloat(css.borderTopWidth) + parseFloat(css.paddingTop),
            bottom: body.getBoundingClientRect().bottom + (parseFloat(style(body).marginBottom) || 0) - bounds.bottom + parseFloat(css.borderBottomWidth) + parseFloat(css.paddingBottom),
        };
    };
    const previous = doc.defaultView.brevladaSpacing;
    if (previous) {
        if (previous.width === viewport && previous.text === body.textContent && previous.entries.every(entry => {
            const current = gaps(entry.owner);
            return Object.keys(current).every(edge => current[edge] === entry.gaps[edge]);
        })) return;
        restoreMessageSpacing(body, previous);
    }
    const nodes = [];
    const walker = doc.createTreeWalker(body, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
        const node = walker.currentNode;
        if (!node.textContent.trim() || node.parentElement.closest('script, style')) continue;
        const css = style(node.parentElement);
        if (css.visibility !== 'visible' || css.opacity === '0') continue;
        const range = doc.createRange();
        range.selectNodeContents(node);
        if ([...range.getClientRects()].some(rect => rect.width && rect.height)) nodes.push(node);
    }
    const minimum = parseFloat(style(body).getPropertyValue('--brevlada-message-inset'));
    const inline = node => node.nodeType === Node.TEXT_NODE || (node.nodeType === Node.ELEMENT_NODE && ['inline', 'contents'].includes(style(node).display) && !node.matches('img, svg, video, audio, iframe, object, canvas, script, style'));
    const owners = new Set();
    for (const node of nodes) {
        let owner = node.parentElement;
        while (['inline', 'contents'].includes(style(owner).display)) owner = owner.parentElement;
        while (owner.parentElement && ['flex', 'inline-flex', 'grid', 'inline-grid'].includes(style(owner.parentElement).display)) owner = owner.parentElement;
        const gap = gaps(owner);
        if (Object.values(gap).every(value => value >= minimum)) {
            owners.add(owner);
            continue;
        }
        const width = owner.getBoundingClientRect().width;
        const containsLayout = owner === body || owner.id === 'brevlada-content' || (!['flex', 'inline-flex', 'grid', 'inline-grid'].includes(style(owner).display) && [...owner.children].some(child => {
            const css = style(child);
            return css.display !== 'none' && (!['inline', 'contents'].includes(css.display) || (child.matches('img, svg, video, iframe, canvas') && child.getBoundingClientRect().width >= width));
        }));
        if (containsLayout) {
            let first = node;
            while (first.parentNode !== owner) first = first.parentNode;
            while (first.previousSibling && inline(first.previousSibling)) first = first.previousSibling;
            const wrapper = doc.createElement('div');
            wrapper.dataset.brevladaSpacingWrapper = 'true';
            owner.insertBefore(wrapper, first);
            while (wrapper.nextSibling && inline(wrapper.nextSibling)) wrapper.appendChild(wrapper.nextSibling);
            owner = wrapper;
        }
        owners.add(owner);
    }
    const entries = [...owners].map(owner => ({owner, originalStyle: owner.getAttribute('style')}));
    const inset = (entry, edge, gap) => {
        if (gap >= minimum) return;
        const property = 'padding' + edge;
        const padding = parseFloat(style(entry.owner)[property]) || 0;
        entry.owner.style[property] = (padding + Math.ceil(minimum - gap)) + 'px';
    };
    for (const entry of entries) {
        const horizontal = gaps(entry.owner);
        if (horizontal.left < minimum || horizontal.right < minimum) {
            entry.owner.style.width = entry.owner.getBoundingClientRect().width + 'px';
            entry.owner.style.boxSizing = 'border-box';
        }
        inset(entry, 'Left', horizontal.left);
        inset(entry, 'Right', horizontal.right);
    }
    for (const entry of entries) inset(entry, 'Top', gaps(entry.owner).top);
    for (const entry of entries.slice().reverse()) inset(entry, 'Bottom', gaps(entry.owner).bottom);
    entries.forEach((entry, index) => {
        if (entry.owner.getAttribute('style') !== entry.originalStyle) entry.owner.dataset.brevladaInset = String(index);
        entry.gaps = gaps(entry.owner);
    });
    doc.defaultView.brevladaSpacing = {width: viewport, text: body.textContent, entries};
}
function cloneMessageContent(doc) {
    const clone = doc.body.cloneNode(true);
    const state = doc.defaultView.brevladaSpacing;
    if (state) restoreMessageSpacing(clone, state);
    return [...clone.childNodes];
}
