let generalStream = new EventSource("api/live_updates", {
    withCredentials: true
});
let latest_data = {};
generalStream.onmessage = (event) => {
    let data = JSON.parse(event.data);
    console.log(data)
    latest_data = data;

    document.dispatchEvent(new CustomEvent("status_update", {
        detail: data,
        bubbles: true,
        cancelable: false
    }));
};
generalStream.onerror = (error) => {
    let data = {
        status: "Disconnected",
        status_colour: "#4d4d4d",
        status_online: false,
        players_online: 0,
        players: [],
        max_players: latest_data.max_players,
        tps: -1
    }
    latest_data = data;

    document.dispatchEvent(new CustomEvent("status_update", {
        detail: data,
        bubbles: true,
        cancelable: false
    }));
    console.error("Error getting live updates:", error);
};

function createElement(data) {
    const temp = document.createElement("div");
    temp.innerHTML = data;
    return temp.firstElementChild;
}

function replaceElement(element, data) {
    let out = createElement(data);
    element.parentElement.replaceChild(out, element);
    return out;
}

function groupSvg(element) {
    let html = element.innerHTML;
    element.innerHTML = "<g></g>";
    element.firstElementChild.innerHTML = html;
    return element.firstElementChild;
}