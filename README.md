## Multiplayer Pokemon Streaming - 🎦 🔴 [View Here](https://pwhiddy.github.io/pokerl-map-viz/)

Stream multiple games of pokemon onto a shared map!  
  

<a href="pwhiddy.github.io/pokerl-map-viz/">
    <img src="/assets/demo.gif?raw=true" height="384">
</a>

## To broadcast:
Use the StreamWrapper gym environment wrapper:
https://github.com/PWhiddy/PokemonRedExperiments/blob/master/baselines/stream_agent_wrapper.py  

And wrap your environment like this:
```python
env = StreamWrapper(
            env, 
            stream_metadata = { # All of this is part is optional
                "user": "super-cool-user", # choose your own username
                "env_id": id, # environment identifier
                "color": "#0033ff", # choose your color :)
                "extra": "", # any extra text you put here will be displayed
            }
        )
```

### Credits
@ Death Strike Gaming - created the complete map!
