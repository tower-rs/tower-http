Request Modifier

HTTP specific Tower middleware that runs a `Fn(Request<B>) -> Request<B>`
modification on every request. This type's builder includes functions for common
modifications, like setting request origin or adding static headers.