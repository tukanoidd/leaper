run_ws *args:
    cargo run -p leaper --features db-websocket -- {{args}}

testbed which *release:
    cargo run -p leaper {{release}} --features testbed-{{which}}

testbed_log which log *release:
    cargo run -p leaper {{release}} --features testbed-{{which}} -- --{{log}}

db:
    surreal start --unauthenticated

profile *args:
    cargo run -p leaper --features profile -- {{args}}

profile_ws *args:
    cargo run -p leaper --features db-websocket,profile -- {{args}}

zipkin:
    docker run -d -p 9411:9411 openzipkin/zipkin