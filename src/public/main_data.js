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
    console.error("Error in EventSource:", error);
};