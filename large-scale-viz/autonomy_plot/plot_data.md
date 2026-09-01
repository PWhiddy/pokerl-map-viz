Title: AI Autonomy
X axis: Independence from human effort
Y axis: Independence from human knowledge
(scale is 0-1, but there are no numerical labels on either axis, coordinates are provided for positioning only
third coord is how filled in the point/circle should be - 0.0 thin ring, 1.0 filled in circle. there should be a legend for this at the bottom too

Create an manim script which animates each of these points being added to the plot one at a time. maybe a 3 second transition for each.
Create a structured file like a json or similar below so I can just edit any of the data below and rerun without any code changes.

Data points:
Random Program [0.95, 0.95, 0.15]
TAS [0.05, 0.05, 1.0]
RL from scratch (Exploration only) [0.4, 0.85, 0.3]
RL (Basic rewards) [0.3, 0.7, 0.4]
LLM (Piman) [0.8, 0.2, 0.1]
RL (Boey) [0.2, 0.4, 1.0] <- this one should be rendered as an X rather than a circle
RL (Rubinstein) [0.15, 0.4, 0.95]
LLM (Hershey, Claude sonnet 3.7) [0.6, 0.15, 0.5]
LLM (Zhang, Gemini pro 2.5) [0.35, 0.1, 1.0]
LLM (Fable, Sol 5.6) [0.8, 0.1, 1.0]
