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

//Magic! Iso time to x days ago etc
function getRelativeTimeString(isoString) {
    const inputDate = new Date(isoString);
    const now = new Date();

    const diffMs = inputDate.getTime() - now.getTime();

    const min = 60 * 1000;
    const hour = 60 * min;
    const day = 24 * hour;
    const week = 7 * day;
    const month = 30 * day;
    const year = 365 * day;

    const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });

    const absDiff = Math.abs(diffMs);

    if (absDiff < min) {
        return rtf.format(Math.round(diffMs / 1000), 'second');
    } else if (absDiff < hour) {
        return rtf.format(Math.round(diffMs / min), 'minute');
    } else if (absDiff < day) {
        return rtf.format(Math.round(diffMs / hour), 'hour');
    } else if (absDiff < week) {
        return rtf.format(Math.round(diffMs / day), 'day');
    } else if (absDiff < month) {
        return rtf.format(Math.round(diffMs / week), 'week');
    } else if (absDiff < year) {
        return rtf.format(Math.round(diffMs / month), 'month');
    } else {
        return rtf.format(Math.round(diffMs / year), 'year');
    }
}