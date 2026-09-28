let logsStream = new EventSource("%base%/logs-stream", {
    withCredentials: true
});
logsStream.onmessage = (event) => {
    console.log("Received message:", event.data);
    let setScroll = false;
    let oldHeight = document.getElementById("logDiv").clientHeight;
    if((document.getElementById("logDiv").scrollTop) / (document.getElementById("logDiv").scrollHeight - document.getElementById("logDiv").clientHeight) === 1) setScroll = true;

    const lines = event.data.replaceAll("\\n", "\n").split("\n");

    lines.forEach((line) => {
        let style = "";
        if (line.startsWith("$0")) {
            line = line.substring(2);
            style = "color: lime";
        } else if (line.startsWith("$1")) {
            line = line.substring(2);
            style = "color: orange";
        } else if (line.startsWith("$2")) {
            line = line.substring(2);
            style = "color: red; background: #5d0202";
        } else if (line.startsWith("$3")) {
            line = line.substring(2);
            style = "color: red";
        } else if (line.startsWith("$4")) {
            line = line.substring(2);
            style = "color: yellow";
        } else if (line.startsWith("$5")) {
            line = line.substring(2);
            style = "blue";
        }
        let element = document.createElement('p');
        element.textContent = line;
        element.style = style;
        document.getElementById('logDiv').appendChild(element);
    })

    if(oldHeight !== document.getElementById("logDiv").scrollHeight && !document.getElementById("logDiv").dataset.hasScrolled) {
        setScroll = true;
        document.getElementById("logDiv").dataset.hasScrolled = true;
    }
    if(setScroll) document.getElementById("logDiv").scrollTop = 9999999999;
};
logsStream.onerror = (error) => {
    console.error("Error in EventSource:", error);
};
