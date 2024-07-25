# block-sleep

```
Block your system from sleeping for an amount of time, or until a certain process exits

Usage: block-sleep [OPTIONS]

Options:
  -p, --pid <PID>       The process id to wait on. Sleep will be blocked until this process exits
  -f, --first <PID>...  Block sleep until the first process in the group has exited
  -a, --all <PID>...    Block sleep until all the given processes in the group have exited
  -t, --time <TIME>     The amount of time to block sleep for in seconds
  -h, --help            Print help
  -V, --version         Print version
```
