    let sidebar_open = localStorage.getItem("sidebar_open") || true;
    async function set_sidebar(v) {
        sidebar_open = v;
        if(v === true) {
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