const formatCommands = ['bold', 'italic', 'underline', 'strikeThrough'];
function reportFormats() {
    window.webkit.messageHandlers.composeFormats.postMessage(JSON.stringify(
        formatCommands.map(command => document.queryCommandState(command))
    ));
}
function applyFormat(command) {
    document.body.focus({preventScroll: true});
    if (command === 'removeFormat' && window.getSelection().isCollapsed) {
        for (const format of formatCommands) {
            if (document.queryCommandState(format)) document.execCommand(format);
        }
    } else {
        document.execCommand(command);
    }
    reportFormats();
    report(true);
}
document.addEventListener('selectionchange', reportFormats);
document.body.addEventListener('input', reportFormats);
