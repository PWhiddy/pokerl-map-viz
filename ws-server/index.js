const express = require('express');
const expressWs = require('express-ws');


console.log("node version:");
console.log(process.version);


const app = express();
expressWs(app);

const broadcasters = new Set();
const receivers = new Set();

const testEchoClients = new Set();

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
    //console.log(Object.keys(message));
    try {
      message = JSON.parse(message);
    } catch (err) {
      console.warn("Invalid JSON received:", err.message);
      return;
    }       
    if (message["metadata"] && message["coords"] && message["metadata"]["user"] !== "") {
            receivers.forEach(receiver => {
              //if (receiver.readyState === WebSocket.OPEN) {
              receiver.send(JSON.stringify(message));
              //}
            });
    }
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

app.ws('/ws-test', (ws, req) => {
  testEchoClients.add(ws);
  
  ws.on('close', () => {
    testEchoClients.delete(ws);
  });

  ws.on('message', (message) => {
    testEchoClients.forEach(ec => {
      try {
        ec.send(message);
        //console.log(message);
      } catch (e) {
        console.log("send ws-test error : ", e);
      }
    });
  });
  
});

const port = 3344;
app.listen(port, () => console.log(`WebSocket server started on ws://localhost:${port}`));
