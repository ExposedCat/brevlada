(() => {
    const root = document.getElementById('brevlada-content');
    if (!root || window.brevladaLayout) return;
    let frame = 0;
    let previous = -1;
    const measure = () => {
        frame = 0;
        const bounds = root.getBoundingClientRect();
        const body = document.body.getBoundingClientRect();
        const bottom = parseFloat(getComputedStyle(document.body).paddingBottom) || 0;
        const height = Math.ceil(bounds.top - body.top + Math.max(bounds.height, root.scrollHeight) + bottom);
        if (height > 0 && height !== previous) {
            previous = height;
            window.webkit.messageHandlers.bodySize.postMessage(height);
        }
    };
    const schedule = () => {
        if (!frame) frame = setTimeout(measure, 0);
    };
    const resize = new ResizeObserver(schedule);
    resize.observe(root);
    const mutation = new MutationObserver(schedule);
    mutation.observe(root, {childList: true, subtree: true, attributes: true, characterData: true});
    document.addEventListener('toggle', schedule, true);
    document.addEventListener('load', schedule, true);
    window.addEventListener('resize', schedule);
    if (document.fonts) document.fonts.ready.then(schedule);
    window.brevladaLayout = {resize, mutation};
    measure();
    schedule();
})();
