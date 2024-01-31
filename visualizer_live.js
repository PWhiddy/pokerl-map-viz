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
        if (container.scale.x < 2.0) {
            backgroundSmooth.visible = true;
            backgroundSharp.visible = false;
        } else if (container.scale.x > 2.0) {     
            backgroundSmooth.visible = false;
            backgroundSharp.visible = true;
        }
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


let coordConversionFunc = (coords) => [0,0];

fetch('assets/map_data.json')
    .then(response => response.json())
    .then(data => {
        MAP_DATA = data.regions.reduce((acc, e) => {
            acc[e.id] = e;
            return acc;
          }, {});
        coordConversionFunc = (coords) => {
            const mapX = MAP_DATA[coords[2]].coordinates[0];
            const mapY = MAP_DATA[coords[2]].coordinates[1];//-vec2(217.5,221.5)
            return [coords[0] + mapX - 217.5, coords[1] + mapY - 221.5];
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

   // "kanto_big_done1.png",
   // "characters_transparent.png",
   // "characters_front.png"

PIXI.Assets.load([
    "assets/kanto_big_done1.png",
    "assets/characters_transparent.png",
    "assets/characters_front.png"
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
    backgroundSharp.visible = false;

    container.addChild(backgroundSmooth);
    container.addChild(backgroundSharp);

        // Function to initialize WebSocket connection
    function initializeWebSocket() {
        const ws = new WebSocket('wss://poke-ws-test-ulsjzjzwpa-ue.a.run.app/receive');
        ws.onmessage = function(event) {
            const data = JSON.parse(event.data); // Assuming the data is JSON-encoded
            const path = data["coords"];
            const meta = data["metadata"];
            console.log(meta);
            if (Date.now() - lastFrameTime < animationDuration) {
                startAnimationForPath(path, meta);
            }
        };
        return ws;
    }

    socket = initializeWebSocket();

    // Refresh WebSocket connection every 2 minutes (120000 milliseconds)
    setInterval(() => {
        console.log("Refreshing WebSocket connection.");
        socket.close(); // Close the current connection
        socket = initializeWebSocket(); // Reinitialize the connection
    }, 120000);

    let baseTextureChar = new PIXI.BaseTexture("assets/characters_transparent.png", {
        // mipmap: PIXI.MIPMAP_MODES.ON
    });

    const charOffset = 1; // 1 index here gets sprite direction index
    let textureChar = getSpriteByCoords(charOffset, 0, baseTextureChar);

    let activeSprites = [];

    function startAnimationForPath(path, meta) {
        const sprite = new PIXI.Sprite(textureChar);
        //sprite.x = charOffset * 40; 
        sprite.anchor.set(0.5);
        //sprite.scale.set(0.5); // Adjust scale as needed
        const subContainer = new PIXI.Container();

        subContainer.addChild(sprite);

        // Check if meta is defined and has a 'user' key
        if (meta && meta.user !== undefined && typeof(meta.user) === "string") {

            meta.user = meta.user.replace(/[^a-zA-Z0-9]/g, ''); // remove non-alphanumeric characters
            const usermatch = meta.user === document.getElementById("hlname").value; // highlight user

            // Create a text label
            const envID = meta.env_id !== undefined ? `-${meta.env_id}` : "";
            const extraInfo = meta.extra !== undefined ? ` ${meta.extra}` : "";
            const color = (meta.color && CSS.supports('color', meta.color)) ? meta.color : "0x000000";
            const label = new PIXI.Text(
                meta.user + envID + extraInfo, 
                {
                    fontFamily: 'Arial',
                    fontSize: usermatch === true ? 128 : 16,
                    fill: color,
                    align: 'center',
            });
            label.x = sprite.x + sprite.width * 0.5; // Position the label next to the sprite
            label.y -= sprite.height; // Adjust the label position as needed
            subContainer.addChild(label);
        }

        container.addChild(subContainer);

        activeSprites.push({ subContainer, path, startTime: null });
    }

    function animate(time) {
        activeSprites.forEach(obj => {
            if (!obj.startTime) obj.startTime = time;
            const timeDelta = time - obj.startTime;
            const progress = Math.min(timeDelta / animationDuration, 1);

            // Calculate the current position
            const currentIndex = Math.floor(progress * (obj.path.length - 1));
            const nextIndex = Math.min(currentIndex + 1, obj.path.length - 1);
            const pointProgress = (progress * (obj.path.length - 1)) - currentIndex;

            const currentPoint = coordConversionFunc(obj.path[currentIndex]);
            const nextPoint = coordConversionFunc(obj.path[nextIndex]);
            obj.subContainer.x = 16*(currentPoint[0] + (nextPoint[0] - currentPoint[0]) * pointProgress);
            obj.subContainer.y = 16*(currentPoint[1] + (nextPoint[1] - currentPoint[1]) * pointProgress);

            if (progress >= 1) {
                container.removeChild(obj.subContainer); // Remove sprite from the scene
                obj.subContainer.destroy({ children: true }); // Optional: frees up memory used by the sprite
            }

        });

        // Remove sprites that have completed their animation
        activeSprites = activeSprites.filter(obj => (time - obj.startTime) < animationDuration);
        lastFrameTime = Date.now();
        requestAnimationFrame(animate);
    }

    requestAnimationFrame(animate);
});
