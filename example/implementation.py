
from gymnasium import Env
from stream_agent_wrapper import StreamWrapper

def make_env():
    def _init():

        env = Env() # Replace with your own environment
        env = StreamWrapper(
                    env, 
                    stream_metadata = { 
                        # All of this is part is optional
                        "user": "pw", # choose your own username
                        "env_id": id, # environment identifier
                        "color": "#0033ff", # choose your color :)
                        "extra": "", # any extra text you put here will be displayed
                    }
                )
        return env
    
    return _init

