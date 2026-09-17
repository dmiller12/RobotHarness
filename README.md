# Robot Harness


This repository contains a Rust-based robot harness designed to manage language descriptions for a Vision-Language-Action (VLA) model to control a manipulator.

VLAs are multimodal models that map visual observations and natural language directly to robotic actions. While they accept text prompts as input, they are generally designed to execute atomic tasks rather than complex, multi-step objectives. 
This harness bridges that gap by utilizing a Large Language Model (LLM) as a reasoning layer.
The LLM decomposes a high-level goal into a sequence of atomic prompts that the VLA can reliably execute, and it actively evaluates visual feedback to track hardware progress against that sequence.

The harness provides two primary user commands:   
- `\planner <goal>`: Takes the latest video frame and the high level goal to generate a sequential plan of atomic tasks.
- `\eval_loop`: Continuously compares the initial frame and latest frame to update the plan progress.

The harness carefully manages LLM context to enable local models on modest hardware.
## Demo
<video src="https://github.com/user-attachments/assets/12504391-19d4-407a-a972-fb3aae804069" controls width="100%"></video>
A teleoperated SO101 arm executing a multi-step objective. The local Qwen3.5-4b model generates the sequence of atomic tasks and continuously evaluates visual progress in real time while the user provides the physical control.
