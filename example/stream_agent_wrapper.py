import asyncio
import websockets
import json
import gymnasium as gym

X_POS_ADDRESS, Y_POS_ADDRESS = 0xD362, 0xD361 # Memory Addresses for X and Y position in Pokemon Gen 1 games
MAP_N_ADDRESS = 0xD35E # Memory Address for map number in Pokemon Gen 1 games

class StreamWrapper(gym.Wrapper):
    def __init__(self, env, stream_metadata={}):
        super().__init__(env)
        self.ws_address = "wss://poke-ws-test-ulsjzjzwpa-ue.a.run.app/broadcast"
        self.stream_metadata = stream_metadata
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        self.websocket = self.loop.run_until_complete(
                self.establish_wc_connection()
        )
        self.upload_interval = 200 # How many steps between each upload
        self.stream_step_counter = 0
        self.coord_list = []
        if hasattr(env, "pyboy"):
            self.emulator = env.pyboy
        elif hasattr(env, "game"):
            self.emulator = env.game
        else:
            raise Exception("Could not find emulator!")

    def step(self, action):

        x_pos = self.emulator.get_memory_value(X_POS_ADDRESS)
        y_pos = self.emulator.get_memory_value(Y_POS_ADDRESS)
        map_n = self.emulator.get_memory_value(MAP_N_ADDRESS)
        self.coord_list.append([x_pos, y_pos, map_n])

        if self.stream_step_counter >= self.upload_interval:
            self.loop.run_until_complete(
                self.broadcast_ws_message(
                    json.dumps(
                        {
                          "metadata": self.stream_metadata,
                          "coords": self.coord_list
                        }
                    )
                )
            )
            self.stream_step_counter = 0
            self.coord_list = []

        self.steam_step_counter += 1

        return self.env.step(action)

    async def broadcast_ws_message(self, message):
        if self.websocket is None:
            await self.establish_wc_connection()
        if self.websocket is not None:
            try:
                await self.websocket.send(message)
            except websockets.exceptions.WebSocketException as e:
                self.websocket = None

    async def establish_wc_connection(self):
        try:
            self.websocket = await websockets.connect(self.ws_address)
        except:
            self.websocket = None