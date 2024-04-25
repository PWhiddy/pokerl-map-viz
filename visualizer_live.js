// Enables point filtering
//PIXI.settings.SCALE_MODE = PIXI.SCALE_MODES.NEAREST;

// Create a PixiJS application
const app = new PIXI.Application({
  resizeTo: window,
  eventMode: "static",
  eventFeatures: {
        wheel: true,
        mouse: true,
  }
});

let socket = null;

let lastFrameTime = Date.now();
let curStats = {envs: 0, viewers: 0};

let allowSpriteDirections = false;
let allowAgentsPathStacking = true;

let backgroundSharp = null;
let backgroundSmooth = null;

// animate each batch of updates for 12 seconds
const animationDuration = 12000;

const container = new PIXI.Container();
// scale and center container initially
const renderWidth = window.innerWidth; // or the width of your specific rendering area
const renderHeight = window.innerHeight; // or the height of your specific rendering area
const desiredCenterX = renderWidth / 2;
const desiredCenterY = renderHeight / 2;
container.x = desiredCenterX;
container.y = desiredCenterY;
container.pivot.x = container.width / 2;
container.pivot.y = container.height / 2;
container.scale.set(0.1, 0.1);

app.stage.addChild(container);

// add the view that Pixi created for you to the DOM
document.body.appendChild(app.view);

const zoomSpeed = 0.0015;

function smoothstep(min, max, value) {
    const x = Math.max(0, Math.min(1, (value-min)/(max-min)));
    return x*x*(3 - 2*x);
}

const createLock = () => {
    let lockStatus = false;
    const release = () => {
        lockStatus = false;
    }
    const acquire = () => {
        if (lockStatus == true)
            return false;
        lockStatus = true;
        return true;
    }
    return {
        lockStatus: lockStatus,
        acquire: acquire,
        release: release,
    };
}

let userFilter = new RegExp("");
let activeAgents = {};
let agentsLock = createLock();

function updateAgents(time){
    Object.entries(activeAgents).forEach(([phash, agent]) => {
        agent.updatePath(time);
    });
}
        
function wipeAgents(time){
    agentsLock.acquire();
    if(time!==undefined){
        activeAgents = Object.keys(activeAgents).reduce(function (filtered, phash) {
            if (!activeAgents[phash].waitingDelete && activeAgents[phash].getRelativeTime(time) < animationDuration) filtered[phash] = activeAgents[phash];
            else activeAgents[phash].setPendingDelete();
            return filtered;
        }, {});
    }else{
        activeAgents = Object.keys(activeAgents).reduce(function (filtered, phash) {
            activeAgents[phash].setPendingDelete();
            return filtered;
        }, {});
    }
    agentsLock.release();
}

function setUserFilter(value) {
    userFilter = new RegExp(value);    
    wipeAgents();
}

function toggleSpriteDirections(){
    allowSpriteDirections = !allowSpriteDirections;
    wipeAgents();
}

function toggleAgentsPathStacking(){
    allowAgentsPathStacking = !allowAgentsPathStacking;
    wipeAgents();
}

app.view.addEventListener('wheel', (e) => {
    e.preventDefault();
    const scaleFactor = 1.0 - (e.deltaY * zoomSpeed);

    // Get the mouse position relative to the canvas
    const rect = app.view.getBoundingClientRect();
    const x = (e.clientX - rect.left) * (app.renderer.width / rect.width);
    const y = (e.clientY - rect.top) * (app.renderer.height / rect.height);

    // Calculate the point to scale around
    const point = new PIXI.Point(x, y);
    const localPoint = container.toLocal(point);

    // Scale the container
    container.scale.x *= scaleFactor;
    container.scale.y *= scaleFactor;

    if (backgroundSmooth && backgroundSharp) {
        const val = container.scale.x;
        const start = 2;
        const end = 4.5;
        const smooth = smoothstep(start, end, val);
        backgroundSharp.alpha = Math.pow(smooth, 0.3);
        backgroundSmooth.alpha = Math.pow(1.0 - smooth, 0.3);
    }

    // Calculate the new position of the point
    const newPoint = container.toGlobal(localPoint);
    container.x -= (newPoint.x - point.x);
    container.y -= (newPoint.y - point.y);
});

let dragging = false;
let dragStart = { x: 0, y: 0 };
let dragOffset = { x: 0, y: 0 };

container.on('mousedown', (event) => {
    dragging = true;
    // Get the position of the mouse relative to the container's position
    dragStart = event.data.getLocalPosition(container.parent);
    // Calculate the offset
    dragOffset.x = container.x - dragStart.x;
    dragOffset.y = container.y - dragStart.y;
})
.on('mouseup', () => {
    dragging = false;
})
.on('mouseupoutside', () => {
    dragging = false;
})
.on('mousemove', (event) => {
    if (dragging) {
        // Get the new position of the mouse relative to the container's parent
        const newPosition = event.data.getLocalPosition(container.parent);
        // Apply the offset to get the new container position
        container.x = newPosition.x + dragOffset.x;
        container.y = newPosition.y + dragOffset.y;
    }
});

let lastTouchDistance = null;

function getTouchDistance(touch1, touch2) {
    const dx = touch1.pageX - touch2.pageX;
    const dy = touch1.pageY - touch2.pageY;
    return Math.sqrt(dx * dx + dy * dy);
}

function getMidpoint(touch1, touch2) {
    return {
        x: (touch1.pageX + touch2.pageX) / 2,
        y: (touch1.pageY + touch2.pageY) / 2,
    };
}

app.view.addEventListener('touchmove', (e) => {
    e.preventDefault();
    if (e.touches.length == 2) {
        // distance between the two touches
        const touchDistance = getTouchDistance(e.touches[0], e.touches[1]);
        // midpoint of the two touches in screen coordinates
        const screenMidpoint = getMidpoint(e.touches[0], e.touches[1]);
        if (lastTouchDistance !== null) {
            const scaleFactor = touchDistance / lastTouchDistance;
            const newScale = container.scale.x * scaleFactor;
            // Convert to the container's local coordinate space
            const rect = app.view.getBoundingClientRect();
            const localMidpoint = container.toLocal(new PIXI.Point(screenMidpoint.x - rect.left, screenMidpoint.y - rect.top));
            container.scale.x = newScale;
            container.scale.y = newScale;
            const newLocalMidpoint = container.toGlobal(localMidpoint);
            container.x += screenMidpoint.x - rect.left - newLocalMidpoint.x;
            container.y += screenMidpoint.y - rect.top - newLocalMidpoint.y;
        }
        lastTouchDistance = touchDistance;
    }
}, { passive: false });

app.view.addEventListener('touchend', () => {
    lastTouchDistance = null;
});

// panning
app.view.addEventListener('touchstart', (e) => {
    if (e.touches.length == 1) { // one finger
        dragging = true;
        const touch = e.touches[0];
        dragStart.x = touch.pageX;
        dragStart.y = touch.pageY;
        dragOffset.x = container.x - dragStart.x;
        dragOffset.y = container.y - dragStart.y;
    }
}, { passive: false });

app.view.addEventListener('touchmove', (e) => {
    if (dragging && e.touches.length == 1) {
        const touch = e.touches[0];
        const newPosition = { x: touch.pageX, y: touch.pageY };
        container.x = newPosition.x + dragOffset.x;
        container.y = newPosition.y + dragOffset.y;
    }
}, { passive: false });

app.view.addEventListener('touchend', () => {
    dragging = false;
});


let coordConversionFunc = (coords) => [0,0,4];

fetch('assets/map_data.json')
    .then(response => response.json())
    .then(data => {
        MAP_DATA = data.regions.reduce((acc, e) => {
            acc[e.id] = e;
            return acc;
          }, {});
        coordConversionFunc = (coords) => {
            if (MAP_DATA[coords[2]] !== undefined) {
              const mapX = MAP_DATA[coords[2]].coordinates[0];
              const mapY = MAP_DATA[coords[2]].coordinates[1];//-vec2(217.5,221.5)
              const maxMapX = Math.trunc(MAP_DATA[coords[2]].tileSize[0],16);
              const maxMapY = Math.trunc(MAP_DATA[coords[2]].tileSize[1],16);
              return [Math.max(0,Math.min(maxMapX,coords[0])) + mapX - 217.5, Math.max(0,Math.min(maxMapY,coords[1])) + mapY - 221.5, coords[3] || 4];
            } else {
              console.warn(`No map coordiate location for id: ${coords[2]}`);
              return [0,0,0];
            }
        };
    })
    .catch(error => console.error('Error loading map data:', error));

function getSpriteByCoords(x, y, baseTex) {
    const sx = 9 + 17 * x;
    const sy = 34 + 17 * y;
    const width = 16;
    const height = 16;

    return new PIXI.Texture(baseTex, new PIXI.Rectangle(sx, sy, width, height));
}

function getDirectionalSpritesById(id, sprite_x_count, baseTex) {
    const sx = 0;
    const sy = 16 * id;
    const width = 16 * (sprite_x_count || 1);
    const height = 16;

    return new PIXI.Texture(baseTex, new PIXI.Rectangle(sx, sy, width, height));
}

function getAgentHash(user,stackID){
    return user + "@" + stackID;
}

   // "kanto_big_done1.png",
   // "sprites_transparent.png",
   // "characters_transparent.png", // OLD SPRITES, REMOVED
   // "characters_front.png"

PIXI.Assets.load([
    "assets/kanto_big_done1.png",
    "assets/sprites_transparent.png",
//    "assets/characters_transparent.png",
//    "assets/characters_front.png"
]).then(() => {

    let baseTextureSmooth = new PIXI.BaseTexture("assets/kanto_big_done1.png", {
        mipmap: PIXI.MIPMAP_MODES.ON, scaleMode: PIXI.SCALE_MODES.LINEAR,
    });
    let textureSmooth = new PIXI.Texture(baseTextureSmooth);
    backgroundSmooth = new PIXI.Sprite(textureSmooth);
    backgroundSmooth.anchor.set(0.5);

    let baseTextureSharp = new PIXI.BaseTexture("assets/kanto_big_done1.png", {
        scaleMode: PIXI.SCALE_MODES.NEAREST,
    });
    let textureSharp = new PIXI.Texture(baseTextureSharp);
    backgroundSharp = new PIXI.Sprite(textureSharp);
    backgroundSharp.anchor.set(0.5);
    backgroundSharp.alpha = 0.0;

    container.addChild(backgroundSmooth);
    container.addChild(backgroundSharp);

    // Function to initialize WebSocket connection
    function initializeWebSocket(url) {
        const ws = new WebSocket(url);
        ws.onmessage = function(event) {
            const data = JSON.parse(event.data); // Assuming the data is JSON-encoded
            if ("stats" in data) {
                curStats = data["stats"];
                document.getElementById('envsCount').innerText = `${curStats.envs} Environments Streaming`;
                document.getElementById('viewersCount').innerText = `${curStats.viewers} Viewers Connected`;
            } else {
                const path = data["coords"];
                const meta = data["metadata"];
                ///console.log(meta);
                if (Date.now() - lastFrameTime < 2 * animationDuration) {
                    startAnimationForPath(path, meta);
                }
            }
        };
        return ws;
    }

    const refreshWS = () => {
        console.log("Refreshing WebSocket connection.");
        if (socket !== null) {
            socket.close(); // Close the current connection
        }
        socket = initializeWebSocket("wss://transdimensional.xyz/receive");
    };

    refreshWS();

    // Refresh WebSocket connection every 2 minutes (120000 milliseconds)
    setInterval(refreshWS, 120000);

    let baseTextureChar = new PIXI.BaseTexture("assets/sprites_transparent.png", {
        scaleMode: PIXI.SCALE_MODES.NEAREST,
    });

    let texturesChars = [];
    let texturesCharsDirectional = [];
    
    for(let i = 0; i < 73; ++i){
        texturesCharsDirectional.push(getDirectionalSpritesById(i, 4, baseTextureChar));
        texturesChars.push(getDirectionalSpritesById(i, 1, baseTextureChar));
    }

    class Agent{
        constructor(user, envID, stackID, extraInfo, color, spriteID, path) {
            this.user = user;
            this.envID = Math.abs(parseInt(envID) || 0);
            const curstackID = Math.abs(parseInt(stackID));
            this.stackID = isNaN(curstackID)? Math.floor(Math.random()*2048) : curstackID;
            this.spriteID = 0;
            this.dataBatchIdx = -1;
            this.dataBatches = [];
            this.waitingDelete = false;
            this.animationDuration = animationDuration;
            this.usingSpriteDirections = false;
            this.sprite = null;
            this.changeSprite(spriteID);
            this.sprite.anchor.set(0.5);
            this.subContainer = new PIXI.Container();
            this.subContainer.addChild(this.sprite);
            this.label = new PIXI.Text(this.formatText(extraInfo), {fontFamily: 'Arial', fontSize: 14, fill: color, align: 'center'});
            this.label.x = this.sprite.x + this.sprite.width * 0.5; // Position the label next to the sprite
            this.label.y -= this.sprite.height; // Adjust the label position as needed
            this.subContainer.addChild(this.label);
            container.addChild(this.subContainer);
            this.appendBatch(path, null, null);
        }
        formatText(extraInfo){
            return this.user + "|" + this.envID + extraInfo;
        }
        changeText(extraInfo, color){
            if (this.waitingDelete) return
            if (extraInfo) this.label.text = this.formatText(extraInfo);
            if (color) this.label.style.fill = color;
        }
        allocateSprite(spriteID){
            self.usingSpriteDirections = allowSpriteDirections;
            return self.usingSpriteDirections ? new PIXI.TilingSprite(texturesCharsDirectional[spriteID], 16, 16) : new PIXI.Sprite(texturesChars[spriteID]);
        }       
        changeSprite(spriteID){
            this.spriteID = Math.abs(parseInt(spriteID) || 0) % texturesCharsDirectional.length;
            this.sprite = this.allocateSprite(this.spriteID)
        }
        updateAnimationTime(){
            if (this.waitingDelete || !allowAgentsPathStacking || this.dataBatches[this.dataBatchIdx] === undefined){
                this.animationDuration = animationDuration;
            }else{
                let batchSteps = Math.max(1, this.dataBatches[this.dataBatchIdx].path.length - 1)
                let nextBatchesCount = Math.max(0,this.dataBatches.length - this.dataBatchIdx - 1);
                let totalSteps = batchSteps + nextBatchesCount;
                for(let i = this.dataBatchIdx+1; i <this.dataBatches.length; ++i){
                    totalSteps += this.dataBatches[i].path.length;
                }
                let log2Steps = Math.floor((31 - Math.clz32(totalSteps)));
                if (nextBatchesCount > 3) log2Steps += 1;
                else if (nextBatchesCount > 7) log2Steps += 3;
                if (log2Steps < 5) log2Steps = 1;
                else if (log2Steps < 7) log2Steps = 2;
                this.animationDuration = 400 * batchSteps / log2Steps;
            }
        }
        appendBatch(path, extraInfo, color){
            if (!this.waitingDelete && path !== undefined){
                if (this.dataBatchIdx < 0) this.dataBatchIdx = 0;
                if ((this.dataBatches.length - this.dataBatchIdx) < 10){
                    if (path.length < 2048) path=path.slice(0, 2048);
                    this.dataBatches.push({path, extraInfo, color, startTime: null});
                    this.updateAnimationTime();
                    return true;
                }
            }
            return false;
        }
        getRelativeTime(time){
            return this.dataBatches[this.dataBatchIdx] !== undefined ? (time - (this.dataBatches[this.dataBatchIdx].startTime || time)) || 0 : this.animationDuration + 1;
        }
        setPendingDelete(){
            container.removeChild(this.subContainer); // Remove sprite from the scene
            this.subContainer.destroy({ children: true }); // Optional: frees up memory used by the sprite
            this.waitingDelete = true;            
        }
        updatePath(time){
            if (this.waitingDelete) return
            if (!this.dataBatches[this.dataBatchIdx].startTime) this.dataBatches[this.dataBatchIdx].startTime = time;
            const timeDelta = time - this.dataBatches[this.dataBatchIdx].startTime;
            const progress = Math.min(timeDelta / this.animationDuration, 1);
            // Calculate the current position
            const currentIndex = Math.floor(progress * (this.dataBatches[this.dataBatchIdx].path.length - 1));
            const nextIndex = Math.min(currentIndex + 1, this.dataBatches[this.dataBatchIdx].path.length - 1);
            const pointProgress = (progress * (this.dataBatches[this.dataBatchIdx].path.length - 1)) - currentIndex;

            const currentPoint = coordConversionFunc(this.dataBatches[this.dataBatchIdx].path[currentIndex]);
            const nextPoint = coordConversionFunc(this.dataBatches[this.dataBatchIdx].path[nextIndex]);
            const deltaPoints = [nextPoint[0] - currentPoint[0], nextPoint[1] - currentPoint[1]];
            const absDeltaPoints = [Math.abs(deltaPoints[0]), Math.abs(deltaPoints[1])];
            // Hide subContainer when warping, to prevent fast and noisy movements
            const visible = Math.max(absDeltaPoints[0], absDeltaPoints[1]) < 1.5;
            this.subContainer.visible = visible;
            if (visible){
                if (this.usingSpriteDirections){
                    let direction = Math.abs(parseInt(nextPoint[2]) || 0);
                    if (direction == 4) {
                        direction = progress >= 1 ? 0 : (absDeltaPoints[1] > 0.5 || absDeltaPoints[0] < 0.5 ? (deltaPoints[1] < -0.5 ? 1 : 0) : (deltaPoints[0] > 0.5 ? 3 : 2));
                    }
                    this.subContainer.children[0].tilePosition.x = -16 * (direction % 4);
                }
                this.subContainer.x = 16 * (currentPoint[0] + deltaPoints[0] * pointProgress);
                this.subContainer.y = 16 * (currentPoint[1] + deltaPoints[1] * pointProgress);
            }
            
            if (progress >= 1) {
                this.dataBatchIdx+=1;
                if (this.dataBatches[this.dataBatchIdx] === undefined){
                    this.setPendingDelete();
                }else{
                    this.dataBatches[this.dataBatchIdx].path.unshift(this.dataBatches[this.dataBatchIdx - 1].path.slice(-1)[0]);
                    this.changeText(this.dataBatches[this.dataBatchIdx].extraInfo, this.dataBatches[this.dataBatchIdx].color);
                }
                delete this.dataBatches[this.dataBatchIdx-1];
                this.updateAnimationTime();
            }
        }
    }

    function startAnimationForPath(path, meta) {
        // Check if meta is defined and has ['user', 'env_id'] keys
        if (meta && meta.user !== undefined && typeof(meta.user) === "string" && meta.user.length > 1){
            const envID = meta.env_id !== undefined ? Math.abs(parseInt(meta.env_id)) : NaN;
            const invalidEnvID = isNaN(envID);
            const stackID = !invalidEnvID && allowAgentsPathStacking ? envID : Math.floor(Math.random()*2048);
            const phash = getAgentHash(meta.user, stackID);
            if (userFilter.exec(phash) !== null) {
                console.log(meta);
                const extraInfo = meta.extra !== undefined ? ` ${meta.extra}` : "";
                const color = (meta.color && CSS.supports('color', meta.color)) ? meta.color : "0x000000";
                agentsLock.acquire();
                if (activeAgents[phash] === undefined || activeAgents[phash].waitingDelete) {
                    const spriteID = meta && meta.sprite_id !== undefined && meta.sprite_id >= 0 && meta.sprite_id < 73 ? parseInt(meta.sprite_id) : 0;
                    let agent = new Agent(meta.user, invalidEnvID ? 0 : envID, stackID, extraInfo, color, spriteID, path);
                    activeAgents[phash]=agent;
                } else activeAgents[phash].appendBatch(path, extraInfo, color);
                agentsLock.release();
            }
        };
    }

    function animate(time) {
        updateAgents(time);
        wipeAgents(time);
        lastFrameTime = Date.now();
        requestAnimationFrame(animate);
    }
    requestAnimationFrame(animate);
});
