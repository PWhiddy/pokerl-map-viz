// paste this into the console to perform a smooth zoom
let sc = () => {

    const rect = app.view.getBoundingClientRect();
    const x = app.renderer.width * 0.5;
    const y = app.renderer.height * 0.5;
    const point = new PIXI.Point(x, y);
    const localPoint = container.toLocal(point);

    container.scale.x *= 0.995;
    container.scale.y *= 0.995;

    if (backgroundSmooth && backgroundSharp) {
        const val = container.scale.x;
        const start = 2;
        const end = 4.5;
        const smooth = smoothstep(start, end, val);
        backgroundSharp.alpha = Math.pow(smooth, 0.3);
        backgroundSmooth.alpha = Math.pow(1.0 - smooth, 0.3);
    }

    const newPoint = container.toGlobal(localPoint);
    container.x -= (newPoint.x - point.x);
    container.y -= (newPoint.y - point.y);
    requestAnimationFrame(sc);
}
requestAnimationFrame(sc);
