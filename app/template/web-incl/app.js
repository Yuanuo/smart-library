$(document).ready(function () {
    if (typeof handleOnPrettyIndent !== 'undefined') {
        handleOnPrettyIndent();
    }
    $('a[data-note]').each(function () {
        const $this = $(this);
        if ($this.attr('id').startsWith('inline-')) {
            const noteText = decodeURIComponent($this.attr('data-note'));
            $this.removeAttr("data-note");
            $this.html('<blockquote>' + noteText.replace(/\n/g, '<br>') + '</blockquote>');
            if ($this.attr('id').startsWith("inline-alert-"))
                setTimeout(function () { alert(noteText); }, 500);
        }
    });

    tippy('a[data-note]', {
        allowHTML: true,
        animation: false,
        placement: 'top',
        content: (ele) => decodeURIComponent(ele.getAttribute('data-note'))
    });
});
function getValidSelectionAnchorInfo(outMapOrElseStr = true) {
    const validSelection = getValidSelection();
    if (!validSelection) return null;
    const selectedText = validSelection.toString().trim();
    if (selectedText.length < 1) return null;

    const selectedNode = validSelection.anchorNode;
    let selectedLine;
    $('body > article').traverse(function (node) {
        const selectedNodeHandled = node === selectedNode;
        if (!selectedNodeHandled && node.nodeType === 3 && node.nodeName === 'span'
            && node.getAttribute('class') && node.getAttribute('class').indexOf('lb') !== -1)
            selectedLine = node;
        return selectedNodeHandled; /* break when selectedNode is matched */
    });

    const selectedEle = selectedLine ? $(selectedLine) : selectedNode.nodeType === 3 ? $(selectedNode.parentElement) : $(selectedNode);
    let selector = selectedEle.cssSelectorEx();
    if (!selector) selector = selectedEle.parent().cssSelectorEx();
    if (!selector) return null;
    const map = {
        "anchor": selector,
        "text": selectedText,
        "rangy": rangy.serializeSelection()
    };
    return outMapOrElseStr ? map : JSON.stringify(map);
}

/* ************************************************************************************************************************************* */

