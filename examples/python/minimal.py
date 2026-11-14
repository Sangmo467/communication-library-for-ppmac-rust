import os
import pmac

with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
    with runtime.create_client() as client:
        print(client.state().state)
