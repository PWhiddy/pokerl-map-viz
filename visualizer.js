// Create a PixiJS application
const app = new PIXI.Application({
  resizeTo: window,
  eventFeatures: {
        wheel: true,
        mouse: true,
  }
});

const container = new PIXI.Container();

app.stage.addChild(container);

// add the view that Pixi created for you to the DOM
document.body.appendChild(app.view);

const interactionManager = app.interaction;
//console.log(app.renderer);


const zoomSpeed = 0.004;
/*
window.addEventListener('wheel', (e) => {
    container.scale.x *= 1.0 - (e.deltaY * zoomSpeed);
    container.scale.y *= 1.0 - (e.deltaY * zoomSpeed);
});
*/

app.view.addEventListener('wheel', (e) => {
    e.preventDefault();
    const scaleFactor = 1.0 + (e.deltaY * zoomSpeed);

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

    // Calculate the new position of the point
    const newPoint = container.toGlobal(localPoint);
    container.x -= (newPoint.x - point.x);
    container.y -= (newPoint.y - point.y);
});

let dragging = false;
let dragStart = { x: 0, y: 0 };
let dragOffset = { x: 0, y: 0 };

container.interactive = true;
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

// load the assets and start the scene
PIXI.Assets.load([
    "kanto_big_done1.png"
]).then(() => {
    // initialize background image
    //const background = PIXI.Sprite.from("kanto_big_done1.png");
    let baseTexture = new PIXI.BaseTexture("kanto_big_done1.png", {
      mipmap: PIXI.MIPMAP_MODES.ON
    });
    let texture = new PIXI.Texture(baseTexture);
    let background = new PIXI.Sprite(texture);
    background.anchor.set(0.5);
    container.addChild(background);

    // scale stage container to match the background size
    //app.stage.scale.x = app.view.width / background.width;
    //app.stage.scale.y = app.view.height / background.height;
});
