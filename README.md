# Can be used, but not recommended

## Anyway, what is it?
1. App Launcher (default mode) (custom solution, good enough for me (but planning to improve things as i go))
2. Command Runner (parse with [shlex](https://docs.rs/shlex/), run with [std::process:Command](https://doc.rust-lang.org/std/process/struct.Command.html))
3. Power Menu (integrated code from [waypwr](https://github.com/tukanoidd/waypwr))
4. (Planned) File Finder
5. Maybe more

## Why
Tried many, while the alternatives are good, they're either too bloated, have
styling issues on my setup, or have some design choices i just don't agree with.
Also, it's fun to build my own software.

## Name?
Initially I started this project in hopes of porting [walker](https://github.com/abenz1267/walker) 
(which I like and still use for [bzmenu](https://github.com/e-tho/bzmenu) and [iwmenu](https://github.com/e-tho/iwmenu) and in keep it as a backup until leaper becomes more stable) 
to Rust+Iced, but realized quickly that the architecture of that project just wont work nicely with iced + I found it less fullfilling to port someones code, and decided to figure
out my own approach to the problem, while keeping the name. (walk -> leap, slow -> (blazingly) fast, u get the point)

## How to install?
When I think it's good enough for others to use, will write up a guide,
for now this is just for me, but others can try it out of they want (through the flake or cargo install).

## Contributions?
Until I release 0.1.0 on crates.io, not gonna accept any, since im still
figuring things out.
