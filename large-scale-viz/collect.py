import asyncio
import websockets
import pandas as pd
import os
from datetime import datetime

# Initialize an empty DataFrame to store messages
messages_df = pd.DataFrame(columns=["timestamp", "message"])

# File to store messages
output_file = "messages.csv.gz"

# Counter to track the number of messages
message_counter = 0
batch_size = 10000

async def save_messages():
    global messages_df
    if not messages_df.empty:
        # Append messages to the compressed CSV file
        if os.path.exists(output_file):
            messages_df.to_csv(output_file, mode='a', header=False, index=False, compression='gzip')
        else:
            messages_df.to_csv(output_file, mode='w', header=True, index=False, compression='gzip')
        messages_df = pd.DataFrame(columns=["timestamp", "message"])

async def listen_to_websocket(uri):
    global message_counter
    global messages_df
    while True:
        try:
            async with websockets.connect(uri) as websocket:
                while True:
                    message = await websocket.recv()
                    timestamp = datetime.utcnow().isoformat()
                    new_row = pd.DataFrame({"timestamp": [timestamp], "message": [message]})
                    messages_df = pd.concat([messages_df, new_row], ignore_index=True)
                    message_counter += 1

                    if message_counter >= batch_size:
                        await save_messages()
                        message_counter = 0
                        print("saved batch!")
                        print(timestamp)
        except (websockets.exceptions.ConnectionClosedError, websockets.exceptions.ConnectionClosedOK):
            print("Connection closed, trying to reconnect...")
            await asyncio.sleep(1)  # Wait before reconnecting

        except Exception as e:
            print(f"An error occurred: {e}")
            await asyncio.sleep(1)  # Wait before reconnecting

async def main():
    uri = "wss://transdimensional.xyz/receive"
    await listen_to_websocket(uri)

# Run the main function
asyncio.run(main())

