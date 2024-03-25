const express = require('express');
const expressWs = require('express-ws');


console.log("node version:");
console.log(process.version);


const app = express();
expressWs(app);

const broadcasters = new Set();
const receivers = new Set();

function doGC() {
        if (global.gc) {
            global.gc();
        } else {
            console.log('Garbage collection unavailable.  Pass --expose-gc '
              + 'when launching node to enable forced garbage collection.');
        }
        setTimeout(doGC, 60000);
}

setTimeout(doGC, 60000);

function updateStats() {
  receivers.forEach(receiver => {
      //if (receiver.readyState === WebSocket.OPEN) {
      receiver.send(JSON.stringify({"stats": {envs: broadcasters.size, viewers: receivers.size}}));
      //}
  });
}

setInterval(updateStats, 10000);

app.ws('/broadcast', function(ws, req) {
  broadcasters.add(ws);
  //console.log('A new broadcaster connected.');

  ws.on('message', function(message) {
    //console.log('Broadcasting: %s', message);
    receivers.forEach(receiver => {
      //if (receiver.readyState === WebSocket.OPEN) {
      receiver.send(message);
      //}
    });
  });

  ws.on('close', () => {
    broadcasters.delete(ws);
    //console.log('Broadcaster disconnected');
  });
});

app.ws('/receive', function(ws, req) {
  receivers.add(ws);
  //console.log('A new receiver connected.');

  ws.on('close', () => {
    receivers.delete(ws);
    //console.log('Receiver disconnected');
  });
});

const port = 3344;
app.listen(port, () => console.log(`WebSocket server started on ws://localhost:${port}`));