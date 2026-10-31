# The client's two calls into this module. Both must exist once client_nca_internal does:
#   InsertDeveloperKeyMappings - gameui.SetupInputMappings, after the key mappings are set up (errors swallowed);
#   EnableDevCommands - clientmethod.Recv_EnableDevCommands, which Rasa.NET sends to GMs entering the world
#                       (only ImportError is caught there, so a missing function would raise).

def InsertDeveloperKeyMappings():
    pass


def EnableDevCommands():
    pass
