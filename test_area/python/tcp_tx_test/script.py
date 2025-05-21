import socket
import sys

HOST = '127.0.0.1'  # Address of your Rust server
PORT = 5005         # Port your Rust server is listening on

def main():
    print("Type messages below. Press Ctrl+C to quit.")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.connect((HOST, PORT))
        try:
            for line in sys.stdin:
                line = line.strip()
                if line:
                    s.sendall((line + '\n').encode('utf-8'))
        except KeyboardInterrupt:
            print("\nDisconnected.")
        except Exception as e:
            print(f"Error: {e}")

if __name__ == "__main__":
    main()
