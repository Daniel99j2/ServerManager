let sidebar_open = localStorage.getItem("sidebar_open") === 'true';

async function set_sidebar(v) {
    sidebar_open = v;
    if (v === false) {
        document.getElementById("sidebar").style.transform = "translateX(-110%)"
        document.getElementById("sidebar-enable").style.opacity = "100"
    } else {
        document.getElementById("sidebar").style.transform = "translateX(0)"
        document.getElementById("sidebar-enable").style.opacity = "0"
    }
    localStorage.setItem("sidebar_open", v);
}

set_sidebar(sidebar_open)

async function toggleSidebar() {
    set_sidebar(!sidebar_open)
}

function makePointer(selected) {
        const temp = document.createElement("div");

        temp.innerHTML = `<svg style="height:1em; width:auto; vertical-align:-0.125em" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide lucide-chevron-right preview-icon"><path d="m9 18 6-6-6-6"/></svg>`;

    const display = temp.firstElementChild;
    display.style.position = "absolute";
    display.style.transition = "all 0s";
    display.style.transform = "translateY(0.125em)";

    if(selected) {
        display.style.right = "5px";
        display.style.color = "#ffffff";
    } else {
        display.style.right = "20px";
        display.style.color = "#bbbbbb00";
        setInterval(() => {
            display.style.transition = "all 0.2s ease";
            display.style.right = "5px";
            display.style.color = "#bbbbbb";
    }, 1)
    }

    return display;
}

for (let sidebar_item of document.getElementsByClassName("sidebaritem")) {
    if(window.location.pathname === "/"+sidebar_item.textContent.toLowerCase()) {
        sidebar_item.appendChild(makePointer(true))
    } else {
        let hover = null;
        sidebar_item.onclick = function () {
            window.location.assign(sidebar_item.textContent.toLowerCase());
        }
        sidebar_item.onmouseenter = function () {
            hover = makePointer(false);
            sidebar_item.appendChild(hover)
        }
        sidebar_item.onmouseleave = function () {
            sidebar_item.removeChild(hover);
        }
    }
    sidebar_item.style.cursor = "pointer"
}