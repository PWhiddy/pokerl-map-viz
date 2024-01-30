// Enables point filtering
PIXI.settings.SCALE_MODE = PIXI.SCALE_MODES.NEAREST;

// Create a PixiJS application
const app = new PIXI.Application({
  resizeTo: window,
  eventMode: "static",
  eventFeatures: {
        wheel: true,
        mouse: true,
  }
});

const decompress = async (url) => {
    const ds = new DecompressionStream("gzip");
    const response = await fetch(url);
    const blob_in = await response.blob();
    const stream_in = blob_in.stream().pipeThrough(ds);
    const blob_out = await new Response(stream_in).blob();
    return await blob_out.text();
};

const tSize = 8192;

const container = new PIXI.Container();

app.stage.addChild(container);

// add the view that Pixi created for you to the DOM
document.body.appendChild(app.view);

const zoomSpeed = 0.004;

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

function getSpriteByCoords(x, y, baseTex) {
    const sx = 9 + 17 * x;
    const sy = 34 + 17 * y;
    const width = 16;
    const height = 16;

    return new PIXI.Texture(baseTex, new PIXI.Rectangle(sx, sy, width, height));
}

// load the assets and start the scene
PIXI.Assets.load([
    "kanto_big_done1.png",
    "characters_transparent.png",
    "characters_front.png"
]).then(() => {
    // initialize background image
    let baseTexture = new PIXI.BaseTexture("kanto_big_done1.png", {
      mipmap: PIXI.MIPMAP_MODES.ON
    });
    let texture = new PIXI.Texture(baseTexture);
    let background = new PIXI.Sprite(texture);
    background.anchor.set(0.5);
    container.addChild(background);

    const baseTextureCharacter = new PIXI.BaseTexture(
        "characters_transparent.png",
        {mipmap: PIXI.MIPMAP_MODES.ON});
    [1, 4, 6, 8].forEach(x => {
        const sprite = new PIXI.Sprite(getSpriteByCoords(x, 0, baseTextureCharacter));
        sprite.x = x * 40; // Adjust position as needed
        sprite.y = 0; // Adjust position as needed
        //container.addChild(sprite);
    });

    // global_coords_64_envs_256_steps_76_games.json.gz 
    decompress("global_coords_64_envs_2048_steps_76_games.json.gz").then(
        (data) => {
            xy_data = JSON.parse(data);
            const buffDat = new Int16Array(
                2*tSize*tSize
            );
            const xy = xy_data["flat_xy"];
            for (let i=0; i<xy.length; i++) {
                buffDat[i] = xy[i];
            }
            const dataTexture = PIXI.Texture.fromBuffer(buffDat, tSize, tSize, {
                format: PIXI.FORMATS.RG_INTEGER, // format for 16-bit integer data
                type: PIXI.TYPES.SHORT, // Specify the data type as short
            });

            dataTexture.baseTexture.mipmap = PIXI.MIPMAP_MODES.OFF;

            // Vertex Shader
            const vertexShader = `
#version 300 es
precision mediump float;
precision highp int;

in vec2 aVertexPosition; // Vertex position attribute
in vec2 aTextureCoord;
in float aInstanceId; // Instance ID attribute
uniform mat3 translationMatrix;
uniform mat3 projectionMatrix;
uniform highp isampler2D uOffsetTexture;
uniform float uTimestep;
uniform int uInstanceCount;
uniform float uStepsPerGame;
uniform float uNumEnvs;
uniform float uGameCount;
uniform float uTextureSize;

out vec2 vTextureCoord; // Pass texture coordinates to the fragment shader
out float fsInstanceID;

vec2 fetchOffset(float instanceId, int timestep) {
    float gameBatch = floor(aInstanceId / uNumEnvs);
    float batchOffset = uStepsPerGame * uNumEnvs * gameBatch;
    float envId = mod(aInstanceId, uNumEnvs);
    float index = batchOffset + float(timestep) * uNumEnvs + envId;
    float x = mod(index, uTextureSize);
    float y = floor(index / uTextureSize);
    ivec2 texCoord = ivec2(x, y);
    ivec4 offsetData = texelFetch(uOffsetTexture, texCoord, 0);
    return vec2(offsetData.gr)-vec2(217.5,221.5); // Convert 16-bit ints to float
}

void main() {
    float fr = fract(uTimestep);
    vec2 curOffset = 16.0*fetchOffset(aInstanceId, int(floor(uTimestep)));
    vec2 nextOffset = 16.0*fetchOffset(aInstanceId, int(floor(uTimestep))+1);
    // TODO fix this interpolation! maybe check the data env order isn't scrambled.
    vec2 offset = mix(curOffset, nextOffset, 0.0); // fr
    
    vec3 position = vec3(aVertexPosition + offset, 1.0);
    gl_Position = vec4((projectionMatrix * translationMatrix * position).xy, 0.0, 1.0);
    vTextureCoord = aTextureCoord; // Assuming you need texture coordinates in the fragment shader
    fsInstanceID = aInstanceId;
}            
                `;

            // Fragment Shader
            const fragmentShader = `
#version 300 es
precision mediump float;
precision highp int;

uniform vec4 uColor;
uniform float uTimestep;
uniform highp isampler2D uOffsetTexture;
uniform sampler2D uSpriteDownTexture;
in vec2 vTextureCoord; // Texture coordinates from the vertex shader
in float fsInstanceID;
out vec4 outColor; // Output color

void main() {
    // Simple example: color the fragment based on the uniform uColor
    outColor = texture(uSpriteDownTexture, vTextureCoord);//uColor;
}
                `;
            let currentTimestep = 0;
            const instanceCount = xy_data["envs"] * xy_data["games"];
            const uniforms = {
                uColor: [1, 0, 0, 1], // Example color
                uOffsetTexture: dataTexture,
                uSpriteDownTexture: new PIXI.Texture(
                    new PIXI.BaseTexture(
                        "characters_front.png",
                        {mipmap: PIXI.MIPMAP_MODES.ON}
                        )
                    ),
                uTimestep: currentTimestep,
                uTextureSize: tSize,
                uInstanceCount: instanceCount,
                uStepsPerGame: xy_data["steps"],
                uNumEnvs: xy_data["envs"],
                uGameCount: xy_data["games"]
            };
            // Instance ID attribute
            let instanceIds = new Float32Array(instanceCount);
            for (let i = 0; i < instanceCount; i++) {
                instanceIds[i] = i;
            }
            
            const geometry = new PIXI.Geometry()
                .addAttribute('aVertexPosition', [
                    -8, -8, // First triangle
                    8, -8,
                    -8,  8,
                    -8,  8, // Second triangle
                    8, -8,
                    8,  8
                ], 2)
                .addAttribute('aTextureCoord', [
                    0, 0, // First triangle
                    1, 0,
                    0, 1,
                    0, 1, // Second triangle
                    1, 0,
                    1, 1
                ], 2) // Quad vertices for a sprite
                .addAttribute('aInstanceId', instanceIds, 1, false, PIXI.TYPES.FLOAT, 0, 0, true);
            
            geometry.instanced = true;
            geometry.instanceCount = instanceCount;

            const shader = PIXI.Shader.from(vertexShader, fragmentShader, uniforms);
            const mesh = new PIXI.Mesh(geometry, shader);
            mesh.eventMode = "none";
            container.addChild(mesh);

            app.ticker.add((delta) => {
                shader.uniforms.uTimestep = currentTimestep % xy_data["steps"];
                currentTimestep += 0.08;
            });
    });

});
