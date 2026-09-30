//so that edits on page load arent transitioned
function set_transition() {
    let element = document.createElement('style');
    element.textContent = "* {transition: all 0.2s ease;}";
    document.body.appendChild(element);
}
setTimeout(set_transition, 50)