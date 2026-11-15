//! The real terminal must survive both keyboard Ctrl-C and delivered SIGINT.
#[cfg(unix)]
#[test]
fn interrupting_evaluation_readline_and_loading_keeps_the_repl_usable() {
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(r#"
import os, pty, select, time, signal, fcntl, termios, struct, sys, tempfile, pathlib
binary=sys.argv[1]
folder=tempfile.TemporaryDirectory(prefix="aevia-interrupt-")
for optimized in [False,True]:
    pid,fd=pty.fork()
    if pid==0:
        os.chdir(folder.name)
        os.environ['TERM']='xterm-256color'
        os.environ['NO_COLOR']='1'
        os.execv(binary,[binary,'--color','never']+(['--release'] if optimized else []))
    fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',40,120,0,0))
    transcript=b''
    def expect(needle,timeout=10):
        global transcript
        data=b''; deadline=time.monotonic()+timeout
        while needle not in data and time.monotonic()<deadline:
            if select.select([fd],[],[],.05)[0]:
                try: chunk=os.read(fd,65536)
                except OSError: break
                data+=chunk; transcript+=chunk
        assert needle in data,(needle,transcript[-6000:])
        return data
    def paste(source):
        os.write(fd,b'\x1b[200~'+source.encode()+b'\x1b[201~\r')
    def cancel_draft(source):
        os.write(fd,source)
        time.sleep(.05)
        os.write(fd,b'\x03')
        data=expect(b'\x1b[?2004l')
        if b'aevia> ' not in data.split(b'\x1b[?2004l',1)[1]: expect(b'aevia> ')
    def prompt_after_interrupt(deliver):
        deliver()
        data=expect(b'E0812',5)
        if b'aevia> ' not in data.split(b'E0812',1)[1]: expect(b'aevia> ',5)
        assert os.waitpid(pid,os.WNOHANG)==(0,0),transcript[-6000:]
    try:
        expect(b'aevia> ')
        # Draft cancellation must also work before the signal handler exists.
        cancel_draft(b'let abandoned = 99')
        paste('let keep = 40\nfn spin(n: Int): Int = spin(n + 1)\nfn main(): IO Unit = do\n    println("STARTED")\n    println("{spin(0)}")\n')
        expect(b'STARTED\r\n')
        prompt_after_interrupt(lambda: os.write(fd,b'\x03'))
        os.write(fd,b'keep + 2\r'); expect(b'= 42')
        time.sleep(.05)
        paste('do\n    println("WAITING_FOR_INPUT")\n    read_line()\n')
        expect(b'WAITING_FOR_INPUT\r\n')
        prompt_after_interrupt(lambda: os.write(fd,b'\x03'))
        os.write(fd,b'keep + 3\r'); expect(b'= 43')
        time.sleep(.05)
        paste('do\n    println("ENTER_NAME")\n    name <- read_line()\n    println("READ={name}")\n')
        expect(b'ENTER_NAME\r\n')
        os.write(fd,'h\u00e9llo\r'.encode()); expect('READ=h\u00e9llo\r\n'.encode())
        time.sleep(.05)
        # The installed handler must leave editable draft Ctrl-C to Rustyline.
        cancel_draft(b'let another_abandoned = 100')
        # Roll back a canceled initializer; SIGINT exercises the handler as well
        # as the keyboard delivery used above.
        paste('let canceled_binding = spin(0)\n')
        time.sleep(.2)
        prompt_after_interrupt(lambda: os.kill(pid,signal.SIGINT))
        os.write(fd,b':type canceled_binding\r'); expect(b'error[E0301]')
        time.sleep(.05)
        os.write(fd,b'keep + 4\r'); expect(b'= 44')
        time.sleep(.05)
        slow=pathlib.Path(folder.name)/'Slow.aev'
        slow.write_text('fn stall(n: Int): Int = stall(n + 1)\nlet answer = stall(0)\n')
        os.write(fd,b'import Slow\r')
        time.sleep(.2)
        prompt_after_interrupt(lambda: os.kill(pid,signal.SIGINT))
        slow.write_text('let answer = 42\n')
        os.write(fd,b'import Slow\r'); expect(b'\r\nok\r\n')
        time.sleep(.05)
        os.write(fd,b'Slow.answer\r'); expect(b'= 42')
        time.sleep(.05)
        os.write(fd,b':reset\r'); expect(b'session cleared')
        time.sleep(.05)
        paste('fn loop(n: Int): Int = loop(n + 1)\nfn main(): IO Unit = do\n    println("AFTER_RESET")\n    println("{loop(0)}")\n')
        expect(b'AFTER_RESET\r\n')
        prompt_after_interrupt(lambda: os.write(fd,b'\x03'))
        os.write(fd,b'6 * 7\r'); expect(b'= 42')
        time.sleep(.05)
        os.write(fd,b'\x04')
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            done,status=os.waitpid(pid,os.WNOHANG)
            if done:
                assert os.waitstatus_to_exitcode(status)==0,transcript[-6000:]
                break
            time.sleep(.05)
        else: raise AssertionError(('Ctrl-D did not exit',transcript[-6000:]))
    finally:
        try: os.kill(pid,signal.SIGKILL)
        except ProcessLookupError: pass
        os.close(fd)
folder.cleanup()
print('REPL evaluation, IO input, initializer/import rollback and reset cancellation passed in both modes')
"#)
        .arg(env!("CARGO_BIN_EXE_aevia"))
        .output().expect("Python 3 terminal harness");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
