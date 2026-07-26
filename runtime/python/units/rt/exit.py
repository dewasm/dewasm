class Exit(Exception):
    def __init__(self, code):
        self.code = code
        super().__init__("proc_exit(%d)" % code)
